//! Small shared widget builders: the labeled input, the switch-style selector
//! and the selection-aware text color.

use super::*;

/// One row of the create form: label left, input box right; no hint when empty.
pub(crate) fn draw_labeled_input(ui: &mut Ui, label: &str, value: &mut String, placeholder: &str) {
    ui.horizontal(|ui| {
        ui.label(label);
        let editor = TextEdit::singleline(value).desired_width(creation_input_width());
        if placeholder.is_empty() {
            ui.add(editor);
        } else {
            ui.add(editor.hint_text(placeholder));
        }
    });
}

/// Foreground of a selectable list item: the selected one takes a contrasting
/// color because the default theme selection background matches the text color.
pub(crate) fn selectable_color(skin: &Skin, selected: bool, unselected_color: Color32) -> Color32 {
    if selected {
        contrasting_text(skin.selection_background)
    } else {
        unselected_color
    }
}

/// The settings-panel switch (options and section titles): unlike a bare
/// `selectable_label` it always draws the inactive border, so it reads clickable.
pub(crate) fn draw_switch(ui: &mut Ui, skin: &Skin, selected: bool, label: String) -> Response {
    let text_color = selectable_color(skin, selected, skin.message_text);
    ui.add(
        Button::selectable(selected, RichText::new(label).color(text_color))
            .frame_when_inactive(true)
            .min_size(button_minimum_size(ui)),
    )
}
