//! The button images embedded in the binary. Every shipped icon is pure white,
//! so it is tinted with the theme's `icon_color` at paint time.

use super::*;

/// One embedded button image, named after its file under `assets/images`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IconName {
    AddMember,
    Back,
    Cancel,
    Confirm,
    CreateGroup,
    CreatePrivateChat,
    DeleteAccount,
    Exit,
    More,
    Proceed,
    Register,
    RemoveMember,
    Search,
    Settings,
}

/// This frame's decoded icons as (which icon, its texture): built once by
/// `BaihuaApp::frame_icons` and handed to the draw functions by reference.
pub(crate) type IconFrame = Vec<(IconName, TextureHandle)>;

/// The embedded bytes of one icon: `include_bytes!` because the mobile packages
/// carry no assets directory (the same reason the logo is embedded).
fn icon_bytes(name: IconName) -> &'static [u8] {
    match name {
        IconName::AddMember => include_bytes!("../../assets/images/add_member.png"),
        IconName::Back => include_bytes!("../../assets/images/back.png"),
        IconName::Cancel => include_bytes!("../../assets/images/cancel.png"),
        IconName::Confirm => include_bytes!("../../assets/images/confirm.png"),
        IconName::CreateGroup => include_bytes!("../../assets/images/create_group.png"),
        IconName::CreatePrivateChat => {
            include_bytes!("../../assets/images/create_private_chat.png")
        }
        IconName::DeleteAccount => include_bytes!("../../assets/images/delete_account.png"),
        IconName::Exit => include_bytes!("../../assets/images/exit.png"),
        IconName::More => include_bytes!("../../assets/images/more.png"),
        IconName::Proceed => include_bytes!("../../assets/images/proceed.png"),
        IconName::Register => include_bytes!("../../assets/images/register.png"),
        IconName::RemoveMember => include_bytes!("../../assets/images/remove_member.png"),
        IconName::Search => include_bytes!("../../assets/images/search.png"),
        IconName::Settings => include_bytes!("../../assets/images/settings.png"),
    }
}

/// Longest side of a decoded icon texture (pixels): the sources are around 130
/// pixels, and 96 stays crisp at the largest scale while uploading little.
fn icon_texture_side() -> usize {
    96
}

/// Largest on-screen box for one icon (points), kept under a text button's content
/// so an image never enlarges its button; the aspect ratio is preserved.
pub(crate) fn icon_maximum_size() -> Vec2 {
    Vec2::new(20.0, 16.0)
}

/// Text drawn if an embedded icon ever fails to decode: plain characters only.
fn icon_placeholder(name: IconName) -> &'static str {
    match name {
        IconName::Back => back_button_text(),
        IconName::More => sidebar_button_text(),
        IconName::Settings => settings_button_text(),
        IconName::CreateGroup | IconName::CreatePrivateChat | IconName::AddMember => {
            create_button_text()
        }
        IconName::Search => "O",
        IconName::Cancel | IconName::Exit => "X",
        IconName::Confirm => "OK",
        IconName::Proceed => ">",
        IconName::Register => "R",
        IconName::RemoveMember => "-",
        IconName::DeleteAccount => "D",
    }
}

/// The icon sized and tinted for a button, or None when its texture is missing.
fn icon_image(tint: Color32, icons: &IconFrame, name: IconName) -> Option<egui::Image<'static>> {
    let (_, handle) = icons.iter().find(|(icon_name, _)| *icon_name == name)?;
    Some(
        egui::Image::from_texture(handle)
            .tint(tint)
            .max_size(icon_maximum_size()),
    )
}

/// Gap between the atoms of an icon-only button: the two elastic atoms already
/// carry all the slack, so a text gap would push the button past its own height.
fn grown_icon_gap() -> f32 {
    0.0
}

/// How far a disabled button's icon fades toward the inactive foreground color.
fn disabled_icon_fade() -> f32 {
    0.7
}

/// The icon color of a visible-but-inert button: the theme color faded toward the
/// noninteractive foreground, so the button reads as disabled without vanishing.
fn disabled_icon_color(ui: &Ui, skin: &Skin) -> Color32 {
    let weak = ui.visuals().widgets.noninteractive.fg_stroke.color;
    skin.icon_color.lerp_to_gamma(weak, disabled_icon_fade())
}

/// The button widget with the theme-tinted icon: text buttons keep the icon on the
/// left; icon-only ones grow elastic atoms, since `min_size` left-aligns content.
fn icon_widget(
    tint: Color32,
    icons: &IconFrame,
    name: IconName,
    text: Option<String>,
    minimum: Vec2,
) -> Button<'static> {
    let button = match (icon_image(tint, icons, name), text) {
        (Some(image), Some(text)) => Button::new((image, RichText::new(text))),
        (Some(image), None) => {
            let mut atoms = Atoms::new(Atom::grow());
            atoms.push_right(Atom::from(image));
            atoms.push_right(Atom::grow());
            Button::new(atoms).gap(grown_icon_gap())
        }
        (None, Some(text)) => Button::new(RichText::new(text)),
        (None, None) => Button::new(RichText::new(icon_placeholder(name))),
    };
    button.min_size(minimum)
}

/// A button carrying the theme-tinted icon, at the shared icon-button size.
pub(crate) fn icon_button(
    ui: &mut Ui,
    skin: &Skin,
    icons: &IconFrame,
    name: IconName,
    text: Option<String>,
) -> Response {
    icon_button_enabled(ui, skin, icons, name, text, true)
}

/// The same button in the shared size, held visible but inert while `enabled` is false.
pub(crate) fn icon_button_enabled(
    ui: &mut Ui,
    skin: &Skin,
    icons: &IconFrame,
    name: IconName,
    text: Option<String>,
    enabled: bool,
) -> Response {
    icon_button_sized(
        ui,
        skin,
        icons,
        name,
        text,
        button_minimum_size(ui),
        enabled,
    )
}

/// The same button with a caller-chosen minimum size, for the buttons whose slot
/// is reserved elsewhere (the send button beside the message box).
pub(crate) fn icon_button_sized(
    ui: &mut Ui,
    skin: &Skin,
    icons: &IconFrame,
    name: IconName,
    text: Option<String>,
    minimum: Vec2,
    enabled: bool,
) -> Response {
    let tint = if enabled {
        skin.icon_color
    } else {
        disabled_icon_color(ui, skin)
    };
    let button = icon_widget(tint, icons, name, text, minimum);
    if enabled {
        ui.add(button)
    } else {
        ui.add_enabled(false, button)
    }
}

/// The same button pinned to an absolute rectangle: overlay controls (the notice
/// window's close corner) sit where the panel says, not where the layout row ends.
pub(crate) fn icon_button_rect(
    ui: &mut Ui,
    skin: &Skin,
    icons: &IconFrame,
    name: IconName,
    text: Option<String>,
    rect: Rect,
) -> Response {
    let button = icon_widget(skin.icon_color, icons, name, text, rect.size());
    ui.put(rect, button)
}

/// The same button in the selected style of the login and registration tabs.
pub(crate) fn icon_switch(
    ui: &mut Ui,
    skin: &Skin,
    icons: &IconFrame,
    name: IconName,
    selected: bool,
    label: String,
) -> Response {
    let text_color = selectable_color(skin, selected, skin.message_text);
    let atoms = match icon_image(skin.icon_color, icons, name) {
        Some(image) => Atoms::from_iter([
            Atom::from(image),
            Atom::from(RichText::new(label).color(text_color)),
        ]),
        None => Atoms::from_iter([Atom::from(RichText::new(label).color(text_color))]),
    };
    ui.add(
        Button::selectable(selected, atoms)
            .frame_when_inactive(true)
            .min_size(button_minimum_size(ui)),
    )
}

/// Every icon name in one list: the frame decodes the whole set once and the
/// cache makes every later frame a lookup, so call sites never pick a subset.
pub(crate) fn all_icon_names() -> Vec<IconName> {
    vec![
        IconName::AddMember,
        IconName::Back,
        IconName::Cancel,
        IconName::Confirm,
        IconName::CreateGroup,
        IconName::CreatePrivateChat,
        IconName::DeleteAccount,
        IconName::Exit,
        IconName::More,
        IconName::Proceed,
        IconName::Register,
        IconName::RemoveMember,
        IconName::Search,
        IconName::Settings,
    ]
}

impl BaihuaApp {
    /// The textures for this frame's buttons: decoded once and cached, since a
    /// drawing closure may not touch `self` while the panel is being laid out.
    pub(crate) fn frame_icons(&mut self, context: &Context) -> IconFrame {
        all_icon_names()
            .into_iter()
            .filter_map(|name| {
                let handle = self.avatars.icon_texture(
                    context,
                    &format!("baihua-icon-{name:?}"),
                    icon_bytes(name),
                    icon_texture_side(),
                )?;
                Some((name, handle))
            })
            .collect()
    }
}

/// The decoded icon set on its own, for the tests that draw a panel without an app.
#[cfg(test)]
pub(crate) fn test_icons(context: &Context) -> IconFrame {
    let mut cache = AvatarTextures::default();
    all_icon_names()
        .into_iter()
        .filter_map(|name| {
            let handle = cache.icon_texture(
                context,
                &format!("baihua-icon-{name:?}"),
                icon_bytes(name),
                icon_texture_side(),
            )?;
            Some((name, handle))
        })
        .collect()
}

#[cfg(test)]
mod icon_tests {
    use super::{IconName, icon_button, test_icons};
    use crate::app::room_panel;
    use crate::app::test_support::frame;
    use crate::app::{button_line_height, panel_corner, panel_inset};
    use crate::appearance::Skin;
    use baihua_core::config::Palette;
    use egui::{CentralPanel, Color32, Context, Rect};

    /// What one frame painted around a single button: the button rectangle, the
    /// icon images (their rectangle and tint) and the text spans.
    type ButtonPaint = (Rect, Vec<(Rect, Color32)>, Vec<String>);

    /// Draw one button for one frame and take back what that frame painted.
    fn one_button(skin: &Skin, name: IconName, text: Option<String>) -> ButtonPaint {
        let context = Context::default();
        skin.apply_to(&context);
        let mut drawn: ButtonPaint = (Rect::NOTHING, Vec::new(), Vec::new());
        context
            .run_ui(frame(500.0, 400.0, Vec::new()), |ctx| {
                CentralPanel::default().show(ctx, |ui| {
                    let icons = test_icons(ui.ctx());
                    drawn.0 = icon_button(ui, skin, &icons, name, text.clone()).rect;
                });
                let shapes = ctx.graphics_mut(|graphics| {
                    let mut shapes: Vec<(Rect, Color32, Option<String>)> = Vec::new();
                    if let Some(list) = graphics.get(egui::LayerId::background()) {
                        for entry in list.all_entries() {
                            match &entry.shape {
                                // A rectangle carrying a texture brush is an image.
                                egui::Shape::Rect(shape) if shape.brush.is_some() => {
                                    shapes.push((shape.rect, shape.fill, None));
                                }
                                egui::Shape::Text(text) => shapes.push((
                                    entry.shape.visual_bounding_rect(),
                                    Color32::TRANSPARENT,
                                    Some(text.galley.text().to_string()),
                                )),
                                _ => {}
                            }
                        }
                    }
                    shapes
                });
                for (rect, color, text) in shapes {
                    match text {
                        Some(text) => drawn.2.push(text),
                        None => drawn.1.push((rect, color)),
                    }
                }
            })
            .drop_without_applying_deltas();
        drawn
    }

    /// Every embedded icon decodes: the images really are inside the binary.
    #[test]
    fn every_icon_decodes() {
        let context = Context::default();
        let icons = test_icons(&context);
        assert_eq!(
            icons.len(),
            super::all_icon_names().len(),
            "each icon must decode to a texture, got {} of {}",
            icons.len(),
            super::all_icon_names().len()
        );
    }

    /// A button without text shows only its image, tinted with the theme colour.
    #[test]
    fn icon_only_no_text() {
        let skin = Skin::from(&Palette::built_in());
        let (button_rect, images, texts) = one_button(&skin, IconName::More, None);
        assert!(
            texts.is_empty(),
            "an icon-only button must not paint its placeholder, got {texts:?}"
        );
        assert!(
            images.iter().any(|(rect, color)| {
                button_rect.contains(rect.center()) && *color == skin.icon_color
            }),
            "the icon must be painted whole inside the button in the theme colour, got {images:?} in {button_rect:?}"
        );
    }

    /// A button with text keeps the text and gets the image to the left of it.
    #[test]
    fn text_keeps_icon() {
        let skin = Skin::from(&Palette::built_in());
        let (button_rect, images, texts) =
            one_button(&skin, IconName::Confirm, Some("confirm".to_string()));
        assert!(
            texts.iter().any(|text| text.contains("confirm")),
            "the title must still be painted, got {texts:?}"
        );
        let image_right = images
            .iter()
            .map(|(rect, _)| rect.max.x)
            .fold(button_rect.min.x, f32::max);
        let middle = button_rect.center().x;
        assert!(
            image_right <= middle,
            "the icon must sit on the left of the text, it ends at {image_right} against the middle {middle}"
        );
    }

    /// An icon-only button centers its image: a button widened by `min_size`
    /// left-aligns content that owns no elastic atom (the reported narrow back key).
    #[test]
    fn icon_centered() {
        let skin = Skin::from(&Palette::built_in());
        let (button_rect, images, texts) = one_button(&skin, IconName::Back, None);
        assert!(texts.is_empty(), "an icon-only button paints no text");
        let (image_rect, _) = *images
            .first()
            .expect("the back button must paint its icon image");
        assert!(
            (image_rect.center().x - button_rect.center().x).abs() < 1.5,
            "the icon must sit at the button's horizontal middle: {image_rect:?} in {button_rect:?}"
        );
        assert!(
            (image_rect.center().y - button_rect.center().y).abs() < 1.5,
            "the icon must sit at the button's vertical middle: {image_rect:?} in {button_rect:?}"
        );
    }

    /// The rectangles a frame painted, as (where, corner radius).
    fn painted_rects(context: &Context) -> Vec<(Rect, u8)> {
        context.graphics_mut(|graphics| {
            let mut rects = Vec::new();
            if let Some(list) = graphics.get(egui::LayerId::background()) {
                for entry in list.all_entries() {
                    if let egui::Shape::Rect(shape) = &entry.shape {
                        rects.push((shape.rect, shape.corner_radius.nw));
                    }
                }
            }
            rects
        })
    }

    /// An icon button is exactly as tall as the plain plus button beside the room
    /// list: the size belongs to the button, not to the shared style.
    #[test]
    fn buttons_match_plus() {
        let context = Context::default();
        let skin = Skin::from(&Palette::built_in());
        let mut icon_rect = Rect::NOTHING;
        let mut wanted = 0.0;
        context
            .run_ui(frame(500.0, 400.0, Vec::new()), |ctx| {
                CentralPanel::default().show(ctx, |ui| {
                    let icons = test_icons(ui.ctx());
                    wanted = button_line_height(ui);
                    icon_rect = icon_button(ui, &skin, &icons, IconName::Settings, None).rect;
                });
            })
            .drop_without_applying_deltas();
        assert!(
            (icon_rect.height() - wanted).abs() < 0.6,
            "the icon button must be {wanted} tall, got {icon_rect:?}"
        );
        let menu = creation_menu_rect();
        assert!(
            (menu.height() - icon_rect.height()).abs() < 0.6,
            "the plus button must keep the shared button height, got {menu:?} against {icon_rect:?}"
        );
    }

    /// Draw the create menu for one frame and hand back the button it painted (the
    /// smallest rectangle in the frame, since the menu itself stays closed).
    fn creation_menu_rect() -> Rect {
        let context = Context::default();
        let skin = Skin::from(&Palette::built_in());
        let mut painted: Vec<(Rect, u8)> = Vec::new();
        context
            .run_ui(frame(500.0, 400.0, Vec::new()), |ctx| {
                CentralPanel::default().show(ctx, |ui| {
                    let icons = test_icons(ui.ctx());
                    let page =
                        crate::app::draw_creation_menu(ui, &skin, &icons, "group", "private");
                    assert!(page.is_none(), "a frame without a click must pick nothing");
                });
                painted = painted_rects(ctx);
            })
            .drop_without_applying_deltas();
        let widths: Vec<f32> = painted.iter().map(|(rect, _)| rect.width()).collect();
        painted
            .into_iter()
            .filter(|(rect, _)| rect.height() > 1.0 && rect.width() < 40.0)
            .map(|(rect, _)| rect)
            .next()
            .unwrap_or_else(|| panic!("the plus button must paint its own box, got {widths:?}"))
    }

    /// Draw the room list frame for one frame and take back its painted rectangles
    /// together with their corner radius.
    fn painted_panel_frame() -> Vec<(Rect, u8)> {
        let context = Context::default();
        let skin = Skin::from(&Palette::built_in());
        let mut shapes: Vec<(Rect, u8)> = Vec::new();
        context
            .run_ui(frame(500.0, 400.0, Vec::new()), |ctx| {
                CentralPanel::default().show(ctx, |ui| {
                    room_panel(&skin).show(ui, |ui| {
                        ui.label("rooms");
                    });
                });
                shapes = ctx.graphics_mut(|graphics| {
                    let mut shapes: Vec<(Rect, u8)> = Vec::new();
                    if let Some(list) = graphics.get(egui::LayerId::background()) {
                        for entry in list.all_entries() {
                            if let egui::Shape::Rect(shape) = &entry.shape {
                                shapes.push((shape.rect, shape.corner_radius.nw));
                            }
                        }
                    }
                    shapes
                });
            })
            .drop_without_applying_deltas();
        shapes
    }

    /// The panel background is a rounded rectangle set in from the window edge by
    /// exactly the inset: that gap is what makes the corners visible.
    #[test]
    fn background_rounded() {
        assert!(panel_corner() > 0.0, "the panels need a positive radius");
        assert!(
            panel_inset() > 0,
            "the panels need a gap to show that radius in"
        );
        let corner = panel_corner();
        let shapes = painted_panel_frame();
        let rounded = shapes
            .iter()
            .find(|(_, radius)| *radius as f32 == corner)
            .unwrap_or_else(|| {
                panic!("a background rounded by {corner} must be painted, got {shapes:?}")
            });
        assert!(
            rounded.0.min.x > 0.0 && rounded.0.min.y > 0.0,
            "the rounded background must be set in from the window edge, got {:?}",
            rounded.0
        );
    }
}
