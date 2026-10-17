use super::*;

fn date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, 17).unwrap()
}

fn task(id: u32, parent_id: Option<u32>, dependencies: &[u32], duration: i64) -> Task {
    Task {
        id, name: format!("Task{id}"), assigned_to: "Tester".into(), duration,
        progress: 0, dependencies: dependencies.to_vec(), manual_start_date: None,
        parent_id, start_date: None, end_date: None,
    }
}

fn project(name: &str, tasks: Vec<Task>) -> ProjectData {
    ProjectData {
        project_name: name.into(), project_start_date: date(), project_end_date: None,
        day_offset: 0, tasks, archived: false,
    }
}

fn app(tasks: Vec<Task>) -> App {
    let mut app = App::empty(PathBuf::new());
    app.all_projects.projects.push(project("Test", tasks));
    app.recalculate_schedule();
    app
}

fn key(app: &mut App, code: KeyCode) {
    dispatch_event(app, Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
}

fn ctrl(app: &mut App, c: char) {
    dispatch_event(app, Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)));
}

struct TestData(PathBuf);

impl TestData {
    fn new() -> Self {
        static ID: AtomicU64 = AtomicU64::new(0);
        let root = env::temp_dir().join(format!(
            "gantt-cli-test-{}-{}", std::process::id(), ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn path(&self) -> PathBuf { self.0.join("projects.json") }

    fn write(&self, data: &AllProjectsData) {
        fs::write(self.path(), serde_json::to_vec(data).unwrap()).unwrap();
    }

    fn load(&self, data: &AllProjectsData) -> App {
        self.write(data);
        App::from_path(self.path()).unwrap()
    }
}

impl Drop for TestData {
    fn drop(&mut self) { fs::remove_dir_all(&self.0).unwrap(); }
}

#[test]
fn corrupt_file_is_not_replaced_with_examples() {
    let file = TestData::new();
    let bytes = b"{\"projects\":[{\"project_name\":\"Important\"}";
    fs::write(file.path(), bytes).unwrap();
    assert!(App::from_path(file.path()).is_err());
    assert_eq!(fs::read(file.path()).unwrap(), bytes);
    assert_eq!(fs::read_dir(&file.0).unwrap().count(), 1);
}

#[test]
fn invalid_project_structure_is_rejected_without_writing() {
    let file = TestData::new();
    let mut data = app(vec![]).all_projects;
    for invalid in 0..2 {
        if invalid == 0 { data.projects.clear(); }
        else { data.projects = vec![project("Only", vec![])]; data.active_project_index = 7; }
        file.write(&data);
        let before = fs::read(file.path()).unwrap();
        assert!(App::from_path(file.path()).is_err());
        assert_eq!(before, fs::read(file.path()).unwrap());
    }
}

#[test]
fn cyclic_parent_data_is_rejected_without_recursing() {
    let file = TestData::new();
    let mut data = app(vec![]).all_projects;
    data.projects[0].tasks = vec![task(1, Some(2), &[], 1), task(2, Some(1), &[], 1)];
    file.write(&data);
    let before = fs::read(file.path()).unwrap();
    assert!(App::from_path(file.path()).is_err());
    assert_eq!(before, fs::read(file.path()).unwrap());
}

#[test]
fn duplicate_ids_and_missing_references_are_rejected() {
    let file = TestData::new();
    for tasks in [
        vec![task(1, None, &[], 1), task(1, None, &[], 1)],
        vec![task(1, Some(99), &[], 1)],
        vec![task(1, None, &[99], 1)],
    ] {
        let mut data = app(vec![]).all_projects;
        data.projects[0].tasks = tasks;
        file.write(&data);
        let before = fs::read(file.path()).unwrap();
        assert!(App::from_path(file.path()).is_err());
        assert_eq!(before, fs::read(file.path()).unwrap());
    }
}

#[test]
fn invalid_loaded_duration_is_rejected_without_writing() {
    let file = TestData::new();
    for duration in [0, -1, i64::MAX] {
        let mut data = app(vec![]).all_projects;
        data.projects[0].tasks.push(task(1, None, &[], duration));
        file.write(&data);
        let before = fs::read(file.path()).unwrap();
        assert!(App::from_path(file.path()).is_err());
        assert_eq!(before, fs::read(file.path()).unwrap());
    }
}

#[test]
fn first_run_examples_have_unique_ids_and_a_valid_dependency() {
    let file = TestData::new();
    let app = App::from_path(file.path()).unwrap();
    let tasks = &app.get_current_project().tasks;
    assert_eq!(tasks.iter().map(|t| t.id).collect::<Vec<_>>(), vec![1, 2]);
    assert_eq!(tasks[1].dependencies, vec![1]);
    assert_eq!(tasks[1].start_date, add_days(tasks[0].end_date.unwrap(), 1));
    assert!(App::from_path(file.path()).is_ok());
}

#[test]
fn undoing_added_task_clears_stale_selection() {
    let mut app = app(vec![]);
    key(&mut app, KeyCode::Char('A'));
    key(&mut app, KeyCode::Enter);
    key(&mut app, KeyCode::Char('u'));
    key(&mut app, KeyCode::Char('u'));
    assert!(app.get_current_project().tasks.is_empty());
    assert_eq!(app.table_state.selected(), None);
    key(&mut app, KeyCode::Enter);
    key(&mut app, KeyCode::Esc);
    key(&mut app, KeyCode::Char('a'));
    assert_eq!(app.get_current_project().tasks.len(), 1);
}

#[test]
fn switching_to_empty_project_allows_a_new_task() {
    let file = TestData::new();
    let mut data = app(vec![task(1, None, &[], 1)]).all_projects;
    data.projects.push(project("Empty", vec![]));
    let mut app = file.load(&data);
    key(&mut app, KeyCode::Char('N'));
    assert_eq!(app.current_project_index, 1);
    assert_eq!(app.table_state.selected(), None);
    key(&mut app, KeyCode::Char('a'));
    assert_eq!(app.get_current_project().tasks.len(), 1);
}

#[test]
fn deleting_project_clears_selection_from_the_previous_project() {
    let mut app = app(vec![task(1, None, &[], 1), task(2, None, &[], 1)]);
    app.all_projects.projects.push(project("Empty", vec![]));
    key(&mut app, KeyCode::Char('G'));
    ctrl(&mut app, 'd'); ctrl(&mut app, 'd');
    assert_eq!(app.get_current_project().project_name, "Empty");
    assert_eq!(app.table_state.selected(), None);
    key(&mut app, KeyCode::Enter);
}

#[test]
fn sibling_insertion_and_movement_keep_parent_before_children() {
    let mut app = app(vec![task(1, None, &[], 1), task(2, Some(1), &[], 1), task(3, None, &[], 1)]);
    app.table_state.select(Some(0));
    key(&mut app, KeyCode::Char('a'));
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.get_current_project().tasks[1].name, "Task2");
    key(&mut app, KeyCode::Char('g'));
    key(&mut app, KeyCode::Char('J'));
    let tasks = &app.get_current_project().tasks;
    let parent = tasks.iter().position(|t| t.name == "Task1").unwrap();
    let child = tasks.iter().position(|t| t.name == "Task2").unwrap();
    assert_eq!(child, parent + 1);
    assert_eq!(tasks[child].parent_id, Some(tasks[parent].id));
    assert!(!app.generate_task_display_ids().values().any(|id| id == "?"));
}

#[test]
fn unindenting_a_subtree_keeps_remaining_children_with_their_parent() {
    let mut app = app(vec![
        task(1, None, &[], 1), task(2, Some(1), &[], 1),
        task(3, Some(2), &[], 1), task(4, Some(1), &[], 1),
    ]);
    app.table_state.select(Some(1));
    app.unindent_task();
    let tasks = &app.get_current_project().tasks;
    assert_eq!(tasks.iter().map(|t| t.id).collect::<Vec<_>>(), vec![1, 4, 2, 3]);
    assert_eq!(tasks[2].parent_id, None);
    assert_eq!(tasks[3].parent_id, Some(2));
    assert_eq!(app.table_state.selected(), Some(2));
}

#[test]
fn downstream_task_waits_for_the_final_parent_end() {
    let mut app = app(vec![task(1, None, &[], 1), task(2, Some(1), &[], 10), task(3, None, &[1], 1)]);
    let tasks = &app.get_current_project().tasks;
    assert_eq!(tasks[0].end_date, add_days(date(), 9));
    assert_eq!(tasks[2].start_date, add_days(date(), 10));
    assert_eq!(tasks[0].duration, 10);
    app.recalculate_schedule();
    assert_eq!(app.get_current_project().tasks[2].start_date, add_days(date(), 10));
}

#[test]
fn nested_parents_finish_before_their_dependants_start() {
    let app = app(vec![
        task(1, None, &[], 1), task(2, Some(1), &[], 1),
        task(3, Some(2), &[], 7), task(4, None, &[1], 2),
    ]);
    assert_eq!(app.get_current_project().tasks[3].start_date, add_days(date(), 7));
}

#[test]
fn valid_dependency_chain_has_no_fixed_iteration_limit() {
    let tasks = (1..=150).rev().map(|id| task(id, None, &if id == 1 { vec![] } else { vec![id-1] }, 1)).collect();
    let app = app(tasks);
    let last = app.get_current_project().tasks.iter().find(|t| t.id == 150).unwrap();
    assert_eq!(last.start_date, add_days(date(), 149));
    assert!(!app.status_message.contains("could not be scheduled"));
}

#[test]
fn dependency_cycle_is_reported_without_hanging() {
    let app = app(vec![task(1, None, &[2], 1), task(2, None, &[1], 1)]);
    assert!(app.get_current_project().tasks.iter().all(|t| t.start_date.is_none()));
    assert!(app.status_message.contains("could not be scheduled"));
}

#[test]
fn durations_are_positive_checked_and_support_units() {
    for value in ["0", "-1", "9223372036854775807", "9223372036854775807w", "bad"] {
        assert_eq!(parse_duration_input(value, date()), None, "{value}");
    }
    assert_eq!(parse_duration_input("2w", date()), Some(14));
    assert_eq!(parse_duration_input("3m", date()), Some(90));
    assert_eq!(parse_duration_input("1y", date()), Some(365));
    assert_eq!(parse_duration_input("2", NaiveDate::MAX), None);
}

#[test]
fn invalid_duration_edit_preserves_previous_value() {
    let mut app = app(vec![task(1, None, &[], 3)]);
    app.table_state.select(Some(0)); app.focus_area = FocusArea::Tasks;
    app.selected_task_field = TaskField::Duration;
    for value in ["0", "-2", "9223372036854775807"] {
        app.input_buffer = value.into();
        save_buffer_to_task(&mut app);
        app.recalculate_schedule();
        assert_eq!(app.get_current_project().tasks[0].duration, 3);
    }
}

#[test]
fn child_outline_ids_remain_unique_after_twenty_six_siblings() {
    let mut tasks = vec![task(1, None, &[], 1)];
    tasks.extend((2..=80).map(|id| task(id, Some(1), &[], 1)));
    let app = app(tasks);
    let ids = app.generate_task_display_ids();
    assert_eq!(ids[&2], "1a");
    assert_eq!(ids[&28], "1aa");
    assert_eq!(ids.values().collect::<HashSet<_>>().len(), ids.len());
}

#[test]
fn large_progress_is_clamped_before_narrowing_to_u8() {
    for value in ["101", "255", "256", "100000", "18446744073709551615"] {
        assert_eq!(parse_progress_input(value), Some(100));
    }
    let mut app = app(vec![task(1, None, &[], 1)]);
    app.table_state.select(Some(0)); app.focus_area = FocusArea::Tasks;
    app.selected_task_field = TaskField::Progress; app.input_buffer = "256".into();
    save_buffer_to_task(&mut app);
    assert_eq!(app.get_current_project().tasks[0].progress, 100);
}

#[test]
fn existing_todo_name_can_be_edited_and_undone() {
    let mut app = app(vec![]);
    app.all_projects.todo_list.push(TodoItem { text: "Old".into(), completed: false, subtasks: vec![] });
    key(&mut app, KeyCode::Char('T'));
    key(&mut app, KeyCode::Enter);
    assert!(app.editing_todo_name);
    assert_eq!(app.input_buffer, "Old");
    app.input_buffer = "Renamed".into();
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.all_projects.todo_list[0].text, "Renamed");
    key(&mut app, KeyCode::Char('u'));
    assert_eq!(app.all_projects.todo_list[0].text, "Old");
}

#[test]
fn all_date_editors_accept_four_digit_years() {
    let expected = NaiveDate::from_ymd_opt(2027, 12, 31).unwrap();
    assert_eq!(parse_date_input("12/31/2027"), Some(expected));
    assert_eq!(parse_date_input("12/31/27"), Some(expected));
    let mut app = app(vec![task(1, None, &[], 1)]);
    for field in [ProjectField::StartDate, ProjectField::EndDate] {
        app.focus_area = FocusArea::Project(field); app.input_buffer = "12/31/2027".into();
        save_buffer_to_task(&mut app);
    }
    assert_eq!(app.get_current_project().project_start_date, expected);
    assert_eq!(app.get_current_project().project_end_date, Some(expected));
    app.focus_area = FocusArea::Tasks; app.table_state.select(Some(0));
    app.selected_task_field = TaskField::StartDate; app.input_buffer = "12/31/2027".into();
    save_buffer_to_task(&mut app); app.recalculate_schedule();
    assert_eq!(app.get_current_project().tasks[0].manual_start_date, Some(expected));
    app.selected_task_field = TaskField::EndDate; app.input_buffer = "01/02/2028".into();
    save_buffer_to_task(&mut app);
    assert_eq!(app.get_current_project().tasks[0].duration, 3);
    app.selected_task_field = TaskField::StartDate;
    load_buffer_for_editing(&mut app);
    assert_eq!(app.input_buffer, "12/31/2027");
}

#[test]
fn atomic_save_leaves_no_temporary_files() {
    let file = TestData::new();
    let mut app = App::from_path(file.path()).unwrap();
    app.get_current_project_mut().project_name = "Saved".into();
    app.save_all_projects().unwrap();
    let saved: AllProjectsData = serde_json::from_slice(&fs::read(file.path()).unwrap()).unwrap();
    assert_eq!(saved.projects[0].project_name, "Saved");
    assert_eq!(fs::read_dir(&file.0).unwrap().count(), 1);
}

#[test]
fn automatic_saves_never_overwrite_external_changes() {
    let file = TestData::new();
    let mut original = app(vec![]).all_projects;
    original.projects.push(project("Second", vec![]));
    for operation in 0..3 {
        let mut app = file.load(&original);
        let external = b"changed by another instance";
        fs::write(file.path(), external).unwrap();
        match operation {
            0 => app.add_new_project(),
            1 => app.next_project(),
            _ => app.previous_project(),
        }
        assert_eq!(fs::read(file.path()).unwrap(), external);
        assert_eq!(app.current_project_index, 0);
        assert_eq!(app.all_projects.projects.len(), 2);
        assert!(app.status_message.contains("externally"));
    }
}

#[test]
fn deleting_the_data_file_is_also_a_save_conflict() {
    let file = TestData::new();
    let mut app = App::from_path(file.path()).unwrap();
    fs::remove_file(file.path()).unwrap();
    assert_eq!(app.save_all_projects().unwrap_err().kind(), io::ErrorKind::AlreadyExists);
    assert!(!file.path().exists());
}

#[test]
fn explicit_overwrite_requires_two_consecutive_save_keys() {
    let file = TestData::new();
    let mut app = App::from_path(file.path()).unwrap();
    let external = b"external content";
    fs::write(file.path(), external).unwrap();
    ctrl(&mut app, 's');
    assert!(app.save_conflict_pending);
    assert_eq!(fs::read(file.path()).unwrap(), external);
    key(&mut app, KeyCode::Char('?'));
    key(&mut app, KeyCode::Char('?'));
    ctrl(&mut app, 's');
    assert!(app.save_conflict_pending);
    assert_eq!(fs::read(file.path()).unwrap(), external);
    ctrl(&mut app, 's');
    assert!(!app.save_conflict_pending);
    assert!(serde_json::from_slice::<AllProjectsData>(&fs::read(file.path()).unwrap()).is_ok());
}

#[test]
fn archive_and_project_order_have_independent_undo_steps() {
    let mut app = app(vec![]);
    app.all_projects.projects.push(project("Second", vec![]));
    app.archive_current_project();
    assert!(app.all_projects.projects[0].archived);
    app.undo();
    assert!(!app.all_projects.projects[0].archived);
    assert_eq!(app.current_project_index, 0);
    app.move_project_forward();
    assert_eq!(app.current_project_index, 1);
    app.undo();
    assert_eq!(app.current_project_index, 0);
    assert_eq!(app.all_projects.projects[0].project_name, "Test");
    app.redo();
    assert_eq!(app.current_project_index, 1);
}

#[test]
fn column_and_timeline_settings_can_be_undone() {
    let mut app = app(vec![]);
    key(&mut app, KeyCode::Char('Z'));
    assert!(app.all_projects.compact_timeline);
    key(&mut app, KeyCode::Char('u'));
    assert!(!app.all_projects.compact_timeline);
    key(&mut app, KeyCode::Char('\\'));
    key(&mut app, KeyCode::Enter);
    assert!(!app.all_projects.column_visibility.assigned_to);
    key(&mut app, KeyCode::Char('\\'));
    key(&mut app, KeyCode::Char('u'));
    assert!(app.all_projects.column_visibility.assigned_to);
}

#[test]
fn restoring_after_undoing_deletion_does_not_duplicate_project() {
    let mut app = app(vec![]);
    app.all_projects.projects.push(project("Second", vec![]));
    app.delete_current_project();
    app.undo();
    app.restore_deleted_project();
    assert_eq!(app.all_projects.projects.len(), 2);
    assert!(app.deleted_projects.is_empty());
}

#[test]
fn dates_at_supported_limits_render_without_overflow() {
    for date in [NaiveDate::MIN, NaiveDate::MAX] {
        let mut app = app(vec![task(1, None, &[], 1)]);
        app.get_current_project_mut().project_start_date = date;
        app.get_current_project_mut().day_offset = i64::MAX;
        app.recalculate_schedule();
        let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(160, 40)).unwrap();
        terminal.draw(|frame| ui(frame, &mut app)).unwrap();
    }
}
