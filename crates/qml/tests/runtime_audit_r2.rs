//! Findings R2-1…R2-19 of the second QML runtime audit (`runtime.rs`, the object model and
//! the Controls types). Every fixture in `runtime_audit_r2/` was run by Qt 6.12.0 (PySide6
//! in the `tools/testenv` image: `QQmlApplicationEngine`, offscreen, quit after 500 ms, the
//! engine deleted at exit); its `.qt` file is what Qt printed, and the engine has to print
//! the same lines. `s…` are the audit's fixtures, `p…` further cases of the same finding,
//! `r2_…` an audit fixture without the lines that belong to another finding, `f…` what still
//! differed after the audit. Qt's own warnings that have no file and line (`Cannot append …`,
//! `QQuickItem::setParentItem: …`) are left out of the `.qt` files.
#![cfg(feature = "interp")]

use qml::runtime::Runtime;
use std::cell::RefCell;
use std::path::Path;

thread_local! {
    static CONSOLE: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn console(_level: u8, line: &str) {
    CONSOLE.with(|c| c.borrow_mut().push(line.to_string()));
}

/// What the fixture printed to the console, and the errors the engine reported: the document
/// is loaded, 400 ms of the engine's clock pass, and the root is deleted as Qt's engine
/// deletes it at exit.
fn run(name: &str) -> (String, Vec<String>) {
    let _ = qml::builtins::CONSOLE_SINK.set(console);
    CONSOLE.with(|c| c.borrow_mut().clear());
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/runtime_audit_r2");
    let mut rt = Runtime::new(Some(dir.clone()));
    let root = rt.load_file(&dir.join(format!("{name}.qml"))).unwrap();
    rt.settle();
    for step in 0..=80 {
        rt.tick(f64::from(step) * 5.0);
    }
    rt.world.borrow_mut().doom(root);
    rt.settle();
    let lines = CONSOLE.with(|c| c.borrow_mut().split_off(0));
    (lines.join("\n"), rt.errors())
}

/// The fixture prints what Qt 6.12 printed, and nothing fails.
fn same_as_qt(name: &str) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/runtime_audit_r2");
    let qt = std::fs::read_to_string(dir.join(format!("{name}.qt"))).unwrap();
    let (out, errors) = run(name);
    assert_eq!(out, qt.trim_end_matches('\n'), "{name}");
    assert_eq!(errors, Vec::<String>::new(), "{name}");
}

#[test]
fn r2_1_text_write_resets_the_cursor_and_selection_of_a_text_edit() {
    same_as_qt("s26b_textarea");
}

#[test]
fn r2_2_only_types_with_a_parent_property_have_one() {
    same_as_qt("s17b");
    same_as_qt("s17d");
    // Qt: `s17c.qml:7: TypeError: Cannot read property 'objectName' of null`: the bare name
    // `parent` inside the QtObject is the document root's, whose parent is null.
    let (out, errors) = run("s17c");
    assert_eq!(out, "");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].ends_with("Cannot read property 'objectName' of null"),
        "{errors:?}"
    );
}

#[test]
fn r2_3_nested_objects_see_the_methods_of_the_document_root() {
    same_as_qt("s29_root_method");
    same_as_qt("s29c");
    same_as_qt("s29d");
    same_as_qt("p03_methods");
}

#[test]
fn r2_4_children_data_and_resources_are_replaced_by_an_assignment() {
    same_as_qt("r2_04_children");
}

#[test]
fn r2_5_stack_view_attached_signals_run_inside_push_and_pop() {
    same_as_qt("s44c_stack");
    same_as_qt("p05_stack");
    // push of a list with a Component, replace, pop(null), pop(item): the elements under the
    // top are removed before `depthChanged`.
    same_as_qt("r2_05_stack_ops");
}

#[test]
fn r2_6_an_item_without_a_parent_item_is_not_visible() {
    same_as_qt("r2_06_visible");
}

#[test]
fn r2_7_descendants_get_enabled_changed_with_their_ancestor() {
    same_as_qt("s14_enabled");
}

#[test]
fn r2_8_a_state_without_a_name_is_named_and_applied() {
    same_as_qt("s01_anon_state");
}

#[test]
fn r2_9_button_group_keeps_one_button_checked() {
    same_as_qt("r2_09_group");
}

#[test]
fn r2_10_a_button_copies_the_state_of_its_action() {
    same_as_qt("s13_action");
}

#[test]
fn r2_11_combo_box_display_text_model_and_current_value() {
    same_as_qt("s12b_combo");
    same_as_qt("p11_combo");
}

#[test]
fn r2_12_an_alias_reaches_a_field_of_a_value_type_property() {
    same_as_qt("s19b_alias");
    same_as_qt("r2_12_alias_groups");
}

#[test]
fn r2_13_an_alias_to_an_id_is_read_only() {
    same_as_qt("s19d_alias");
}

#[test]
fn r2_14_destroy_refuses_an_object_the_host_created() {
    same_as_qt("s33_destroy_root");
    same_as_qt("p14_destroy");
    same_as_qt("p14b_destroy");
}

#[test]
fn r2_15_property_value_objects_are_destroyed_before_the_children() {
    same_as_qt("s42_destr_order");
    same_as_qt("p15_destr");
}

#[test]
fn r2_16_a_negative_maximum_length_limits_nothing() {
    same_as_qt("s22_maxlen");
    same_as_qt("p16_maxlen");
}

#[test]
fn r2_17_clearing_the_input_mask_clears_the_text() {
    same_as_qt("r2_17_mask");
}

#[test]
fn r2_18_enum_values_in_hexadecimal() {
    same_as_qt("s31_enum");
}

#[test]
fn r2_19_progress_bar_clamps_its_value_and_mirrors_its_visual_position() {
    same_as_qt("s16_progress");
    same_as_qt("p19b");
}

// What still differed from Qt 6.12 next to the findings above (`f…` fixtures).

#[test]
fn f17_display_text_follows_the_text_and_the_echo_mode() {
    same_as_qt("f17_display");
}

#[test]
fn f17_a_mask_write_notifies_in_the_order_of_qt_with_both_cursor_moves() {
    same_as_qt("f17_mask");
}

#[test]
fn f12_source_size_is_a_value_that_notifies_per_write_and_per_load() {
    same_as_qt("f12_source_size");
}

#[test]
fn f12_a_source_size_group_notifies_once_while_the_object_is_populated() {
    same_as_qt("f12_creation");
}

#[test]
fn f04_an_item_in_a_property_is_no_child_and_the_lists_move_objects_as_qt() {
    same_as_qt("f04_children");
}

#[test]
fn f04_an_item_in_a_property_gets_its_parent_when_the_children_binding_lists_it() {
    same_as_qt("f04_property_child");
}

#[test]
fn f04_the_item_lists_refuse_undefined_and_objects_of_another_type() {
    same_as_qt("f04_list_errors");
}

#[test]
fn f02b_a_binding_object_holds_its_target_while_when_is_true() {
    same_as_qt("f02b_binding");
}

#[test]
fn f01_the_signals_of_an_object_being_created() {
    same_as_qt("f01_creation");
}

#[test]
fn s40_a_reparented_item_and_child_at() {
    same_as_qt("s40_reparent");
    same_as_qt("f40_child_at");
}

#[test]
fn f05_removed_waits_for_the_busy_changed_handler_that_pushes() {
    same_as_qt("f05_stack_removed");
}

#[test]
fn s15_visible_children_lists_the_visible_child_items() {
    same_as_qt("s15_visible");
    same_as_qt("f15_visible_children");
}

#[test]
fn s50_the_border_of_a_rectangle_is_one_pixel_wide_by_default() {
    same_as_qt("f50_border_width");
}

#[test]
fn s19c_an_alias_to_a_group_field_reads_the_default_of_the_group() {
    same_as_qt("s19c_alias");
}
