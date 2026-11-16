use opencode_multiplexer::app::split_picker::{SplitChoice, SplitPickerState};
use std::collections::HashSet;

#[test]
fn shell_is_first_and_sessions_keep_their_order() {
    let choices = opencode_multiplexer::app::split_picker::build_split_choices(
        [(1, "A".to_string()), (2, "B".to_string())],
        &HashSet::new(),
    );

    assert_eq!(choices[0], SplitChoice::Shell);
    assert_eq!(
        choices[1],
        SplitChoice::Session {
            entry_id: 1,
            title: "A".into()
        }
    );
    assert_eq!(
        choices[2],
        SplitChoice::Session {
            entry_id: 2,
            title: "B".into()
        }
    );
}

#[test]
fn split_choices_exclude_sessions_already_in_workspace() {
    let choices = opencode_multiplexer::app::split_picker::build_split_choices(
        [(1, "A".to_string()), (2, "B".to_string())],
        &HashSet::from([1]),
    );

    assert_eq!(
        choices,
        vec![
            SplitChoice::Shell,
            SplitChoice::Session {
                entry_id: 2,
                title: "B".into()
            },
        ]
    );
}

#[test]
fn shell_is_the_only_choice_when_there_are_no_sessions() {
    assert_eq!(
        opencode_multiplexer::app::split_picker::build_split_choices(
            std::iter::empty(),
            &HashSet::new(),
        ),
        vec![SplitChoice::Shell]
    );
}

#[test]
fn shell_is_selected_by_default_when_it_is_first() {
    let choices = opencode_multiplexer::app::split_picker::build_split_choices(
        [(1, "A".to_string())],
        &HashSet::new(),
    );
    let picker = SplitPickerState::new(4, 9, false, choices);

    assert_eq!(picker.selected(), Some(&SplitChoice::Shell));
}

#[test]
fn split_picker_cycles_choices_and_preserves_target_metadata() {
    let mut picker = SplitPickerState::new(
        4,
        9,
        false,
        vec![
            SplitChoice::Session {
                entry_id: 12,
                title: "worker".into(),
            },
            SplitChoice::Shell,
        ],
    );

    assert_eq!(
        picker.selected(),
        Some(&SplitChoice::Session {
            entry_id: 12,
            title: "worker".into()
        })
    );
    picker.move_down();
    assert_eq!(picker.selected(), Some(&SplitChoice::Shell));
    picker.move_down();
    assert_eq!(
        picker.selected(),
        Some(&SplitChoice::Session {
            entry_id: 12,
            title: "worker".into()
        })
    );
    assert_eq!(picker.workspace_id(), 4);
    assert_eq!(picker.target_pane(), 9);
    assert!(!picker.horizontal());
}
