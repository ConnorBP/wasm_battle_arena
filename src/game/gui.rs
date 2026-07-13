use bevy::prelude::*;
use bevy_egui::{egui::*, EguiContexts};

use super::{
    assets::sounds::AudioConfig,
    networking::{sanitize_room_code, MatchmakingRoom},
    session::PlayerProfile,
    GameState, PendingPlayerProfile, Scores,
};

#[derive(States, Clone, Eq, PartialEq, Debug, Hash, Default)]
pub enum MenuState {
    #[default]
    Main,

    DirectConnect,
    // sub menu of DirectConnect
    HostLobby,
    // sub menu of DirectConnect
    JoinLobby,
    Settings,
    #[cfg(feature="sync_test")]
    SyncTest
}

/// Reference viewport size used to scale menu fonts and margins responsively.
const REFERENCE_WIDTH: f32 = 1280.0;
const REFERENCE_HEIGHT: f32 = 720.0;

/// Responsive scale factor (0.6 ..= 1.0) for menu fonts and spacing.
/// Shrinks on viewports smaller than the 1280x720 reference so panels stay usable.
fn responsive_scale(ctx: &Context) -> f32 {
    let size = ctx.screen_rect().size();
    (size.x / REFERENCE_WIDTH)
        .min(size.y / REFERENCE_HEIGHT)
        .clamp(0.6, 1.0)
}

/// Responsive inner margin for the menu panels: comfortable spacing on large
/// screens, but never so large that it crowds out the (scrollable) content on
/// small viewports.
fn panel_margin(ctx: &Context) -> Margin {
    let size = ctx.screen_rect().size();
    let scale = responsive_scale(ctx);
    let horizontal = (size.x * 0.06).clamp(8.0, 40.0) * scale;
    let vertical = (size.y * 0.04).clamp(8.0, 32.0) * scale;
    Margin::symmetric(horizontal, vertical)
}

/// handle keybinds for interacting with menu.
/// Ex. hotkeys for menu toggle
pub fn handle_menu_input(
    keys: Res<Input<KeyCode>>,
    current_menu_state: Res<State<MenuState>>,
    current_game_state: Res<State<GameState>>,
    mut next_menu_state: ResMut<NextState<MenuState>>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        match current_game_state.get() {
            GameState::InGame | GameState::Matchmaking => {
                // check for settings toggle key while  in game
                match current_menu_state.get() {
                    MenuState::Main => {
                        next_menu_state.set(MenuState::Settings);
                    },
                    MenuState::Settings => {
                        next_menu_state.set(MenuState::Main);
                    },
                    _=> {},
                }
            },
            _ => {},
        }
    }
}

pub fn update_main_menu(
    mut contexts: EguiContexts,
    mut next_game_state: ResMut<NextState<GameState>>,
    mut next_menu_state: ResMut<NextState<MenuState>>,
    mut room: ResMut<MatchmakingRoom>,
) {
    let scale = responsive_scale(contexts.ctx_mut());
    TopBottomPanel::top("main menu top")
    .show(contexts.ctx_mut(), |ui| {
        ui.label(
            RichText::new(format!("GHOSTIES {}", env!("CARGO_PKG_VERSION")))
                .color(Color32::LIGHT_BLUE)
                .font(FontId::proportional(52.0 * scale)),
        );

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.label(
                RichText::new("Game by ")
                    .color(Color32::GRAY)
                    .font(FontId::monospace(24.0)),
            );
            ui.hyperlink_to(RichText::new("Connor Postma")
            .font(FontId::monospace(24.0)), "https://github.com/ConnorBP");
            ui.label(
                RichText::new(" 2023")
                    .color(Color32::GRAY)
                    .font(FontId::monospace(24.0)),
            );
        });
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.label(
                RichText::new("Music by ")
                    .color(Color32::GRAY)
                    .font(FontId::monospace(24.0)),
            );
            ui.hyperlink_to(RichText::new("Warren Postma")
            .font(FontId::monospace(24.0)), "https://on.soundcloud.com/bF9zR");
            ui.label(RichText::new(".").font(FontId::monospace(24.0)));
        });
    });
        

    bevy_egui::egui::CentralPanel::default()
    .frame(
        Frame::none()
        .inner_margin(panel_margin(contexts.ctx_mut()))
        .fill(Color32::from_rgb(66, 69, 73))
    )
    .show(contexts.ctx_mut(), |ui| {
        // set spacing
        ui.style_mut().spacing.indent = 16.0;
        ui.style_mut().spacing.item_spacing = vec2(10.0, 10.0 * scale);
        ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.vertical_centered_justified(|ui| {
            // set button style
        if let Some(button_style) = ui.style_mut().text_styles.get_mut(&TextStyle::Button) {
            *button_style = FontId::new(28.0 * scale, FontFamily::Proportional);
        }
        if ui.button("▶ Start Matchmaking").clicked() {
            room.private_code = None;
            next_menu_state.set(MenuState::Main);
            next_game_state.set(GameState::Matchmaking);
        }
        ui.horizontal(|ui| {
            ui.checkbox(&mut room.use_lobby_v2, "Lobby v2 / Deathmatch preview");
            if room.use_lobby_v2 {
                if ui.selectable_label(room.mode == super::session::GameMode::Duel, "Duel").clicked() {
                    room.mode = super::session::GameMode::Duel; room.capacity = 2;
                }
                if ui.selectable_label(room.mode == super::session::GameMode::Deathmatch, "Deathmatch").clicked() {
                    room.mode = super::session::GameMode::Deathmatch; room.capacity = room.capacity.max(3);
                }
            }
        });
        if room.use_lobby_v2 && room.mode == super::session::GameMode::Deathmatch {
            ui.add(Slider::new(&mut room.capacity, 3..=4).text("Players"));
        }
        if ui.button("🔒 Private Match").clicked() {
            next_menu_state.set(MenuState::DirectConnect);
        }
        #[cfg(feature="sync_test")]
        if ui.button("SyncTest").clicked() {
            next_menu_state.set(MenuState::SyncTest);
        }
        if ui.button("⚙ Settings").clicked() {
            next_menu_state.set(MenuState::Settings);
        }
        });
        });
    });
}

pub fn update_direct_connect_ui(
    mut contexts: EguiContexts,
    mut next_game_state: ResMut<NextState<GameState>>,
    mut next_menu_state: ResMut<NextState<MenuState>>,
    mut room: ResMut<MatchmakingRoom>,
    mut code: Local<String>,
) {
    let scale = responsive_scale(contexts.ctx_mut());
    CentralPanel::default()
        .frame(Frame::none().inner_margin(panel_margin(contexts.ctx_mut())).fill(Color32::from_rgb(66, 69, 73)))
        .show(contexts.ctx_mut(), |ui| {
            ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.style_mut().spacing.item_spacing.y = 10.0 * scale;
            ui.vertical_centered_justified(|ui| {
                ui.heading("Private Match");
                ui.label("Enter the same room code on both devices.");
                if ui.text_edit_singleline(&mut *code).changed() {
                    *code = sanitize_room_code(code.as_str());
                }
                if ui.add_enabled(!code.is_empty(), Button::new("Create / Join Private Match")).clicked() {
                    room.private_code = Some(code.clone());
                    next_menu_state.set(MenuState::Main);
                    next_game_state.set(GameState::Matchmaking);
                }
                if ui.button("Back").clicked() {
                    next_menu_state.set(MenuState::Main);
                }
            });
            });
        });
}

pub fn update_in_game_controls_ui(
    mut contexts: EguiContexts,
    mut next_menu_state: ResMut<NextState<MenuState>>,
) {
    Area::new("controls menu")
    .anchor(Align2::LEFT_TOP, (25., 25.))
    .show(contexts.ctx_mut(), |ui| {

        if let Some(button_style) = ui.style_mut().text_styles.get_mut(&TextStyle::Button) {
            *button_style = FontId::new(48.0, FontFamily::Proportional);
        }

        if ui.button("⚙").clicked() {
            next_menu_state.set(MenuState::Settings);
        }
    });
}

pub fn update_settings_ui(
    mut contexts: EguiContexts,
    mut next_menu_state: ResMut<NextState<MenuState>>,
    mut audio_config: ResMut<AudioConfig>,
    mut profile: ResMut<PendingPlayerProfile>,
) {
    let scale = responsive_scale(contexts.ctx_mut());
    bevy_egui::egui::CentralPanel::default()
    .frame(
        Frame::none()
        .inner_margin(panel_margin(contexts.ctx_mut()))
        .fill(Color32::from_rgb(66, 69, 73))
    )
    .show(contexts.ctx_mut(), |ui| {
        // set spacing
        ui.style_mut().spacing.indent = 16.0;
        ui.style_mut().spacing.item_spacing = vec2(16.0, 16.0);

        let wide = ui.available_height() < ui.available_width();
        ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.style_mut().spacing.item_spacing.y = 10.0 * scale;
        ui.vertical_centered_justified(|ui| {
            // set button style
            if let Some(button_style) = ui.style_mut().text_styles.get_mut(&TextStyle::Button) {
                *button_style = FontId::new(28.0 * scale, FontFamily::Proportional);
            }

            let extra_slider_widget_size = {
                if wide {
                    200.
                } else {
                    0.
                }
            };

            // justify the sliders (- 200 for extra display value and text size)
            ui.style_mut().spacing.slider_width = ui.max_rect().width() - extra_slider_widget_size;

            ui.heading("Player Settings");

            let label = ui.label("Player Name: ");
            if ui.text_edit_singleline(&mut profile.name).labelled_by(label.id).changed() {
                profile.name = PlayerProfile::sanitized_name(&profile.name);
                if profile.name.is_empty() { profile.name = "Ghost".into(); }
            }
            ui.horizontal(|ui| {
                ui.label("Color:");
                for (id, color) in [Color32::RED, Color32::BLUE, Color32::GREEN, Color32::from_rgb(190, 70, 190)].into_iter().enumerate() {
                    if ui.selectable_label(profile.palette_id == id as u8, RichText::new("●").color(color)).clicked() {
                        profile.palette_id = id as u8;
                    }
                }
            });
            ComboBox::from_label("Cosmetic")
                .selected_text(["Classic", "Crown", "Wizard", "Bow"][profile.cosmetic_id as usize])
                .show_ui(ui, |ui| {
                    for (id, name) in ["Classic", "Crown", "Wizard", "Bow"].into_iter().enumerate() {
                        ui.selectable_value(&mut profile.cosmetic_id, id as u8, name);
                    }
                });
            ui.small("Applied once lobby profile synchronization is enabled.");

            ui.heading("Volume Settings");
            
            if !wide {
                ui.label("🔊 Master Volume");
            }
            ui.add(
                {
                    let mut slider= Slider::new(&mut audio_config.master_volume, 0.0..=100.0)
                        .show_value(wide)
                        .trailing_fill(true);
                    if wide {slider = slider.text("🔊 Master Volume");}
                    slider
                }
                
            );
            
            if !wide {
                ui.label("🎵 Music Volume");
            }
            ui.add(
                {
                    let mut slider = Slider::new(&mut audio_config.music_volume, 0.0..=100.0)
                        .show_value(wide)
                        .trailing_fill(true);
                    if wide {slider = slider.text("🎵 Music Volume");}
                    slider
                }
            );

            if !wide {
                ui.label("💥 SFX Volume");
            }
            ui.add(
                {
                    let mut slider = Slider::new(&mut audio_config.sfx_volume, 0.0..=100.0)
                        .show_value(wide)
                        .trailing_fill(true);
                    if wide {slider = slider.text("💥 SFX Volume");}
                    slider
                }
            );

            // return to main menu
            if ui.button("Back").clicked() {
                next_menu_state.set(MenuState::Main);
            }
        });
        });
    });
}


pub fn update_score_ui(mut contexts: EguiContexts, scores: Res<Scores>) {
    let score_text = scores
        .entries()
        .iter()
        .map(|entry| entry.score.to_string())
        .collect::<Vec<_>>()
        .join(" : ");

    Area::new("score")
    .anchor(Align2::CENTER_TOP, (0., 25.))
    .show(contexts.ctx_mut(), |ui| {
        ui.label(
            RichText::new(score_text)
                .color(Color32::WHITE)
                .font(FontId::proportional(72.0)),
        );
    });
}

#[cfg(test)]
mod layout_tests {
    use super::*;

    #[test]
    fn responsive_layout_stays_compact_on_mobile() {
        for size in [vec2(390.0, 844.0), vec2(844.0, 390.0)] {
            let ctx = Context::default();
            ctx.set_pixels_per_point(1.0);
            ctx.begin_frame(RawInput { screen_rect: Some(bevy_egui::egui::Rect::from_min_size(Pos2::ZERO, size)), ..default() });
            let margin = panel_margin(&ctx);
            assert!(margin.left <= 40.0 && margin.top <= 32.0);
            assert!((0.6..=1.0).contains(&responsive_scale(&ctx)));
            let _ = ctx.end_frame();
        }
    }
}

pub fn update_matchmaking_ui(mut contexts: EguiContexts) {

    Area::new("matchmaking info")
    .anchor(Align2::CENTER_TOP, (0., 25.))
    .show(contexts.ctx_mut(), |ui| {
        ui.label(
            RichText::new(format!("GHOSTIES {}", env!("CARGO_PKG_VERSION")))
                .color(Color32::LIGHT_BLUE)
                .font(FontId::proportional(68.0)),
        );
        ui.label(
            RichText::new(format!("Game by Connor Postma 2023"))
                .color(Color32::GRAY)
                .font(FontId::monospace(24.0)),
        );
        ui.label(
            RichText::new(format!("Waiting for opponent to join..."))
                .color(Color32::WHITE)
                .font(FontId::proportional(48.0)),
        );
    });
}

pub fn update_respawn_ui(mut contexts: EguiContexts) {
    Area::new("matchmaking info")
    .anchor(Align2::CENTER_CENTER, (0., 25.))
    .show(contexts.ctx_mut(), |ui| {
        ui.label(
            RichText::new(format!("SCORE!\nRespawning..."))
                .color(Color32::WHITE)
                .font(FontId::proportional(32.0)),
        );
        ui.spinner();
    });
}
