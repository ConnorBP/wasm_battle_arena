use crate::mobile_input::{self, MobileInputKind};
use bevy::prelude::*;
use bevy_egui::{egui::*, EguiContexts};
use std::time::{SystemTime, UNIX_EPOCH};

use super::{
    assets::sounds::AudioConfig,
    components::{MarkedForDeath, Player, ShieldCharges, SpeedBoost},
    networking::{sanitize_room_code, LocalPlayerHandle, MatchmakingRoom},
    practice::{PracticeCooldown, PracticeScore},
    progression::{CasualProfile, COSMETICS},
    session::{mode_label, PlayerProfile, RoundBootstrap, MATCH_POINTS_TO_WIN},
    GameState, MatchFlow, PendingPlayerProfile, RematchFlow, RollbackState, Scores,
};
use crate::cloudflare_net::CloudflareSocket;

#[derive(States, Clone, Eq, PartialEq, Debug, Hash, Default)]
pub enum MenuState {
    #[default]
    Main,

    DirectConnect,
    Settings,
    #[cfg(feature = "sync_test")]
    SyncTest,
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

const PANEL_DARK: Color32 = Color32::from_rgb(20, 24, 34);
const PANEL_RAISED: Color32 = Color32::from_rgb(31, 38, 52);
const OUTLINE: Color32 = Color32::from_rgb(104, 224, 238);
const ACCENT: Color32 = Color32::from_rgb(244, 203, 72);
const STATUS_DANGER: Color32 = Color32::from_rgb(255, 92, 108);

/// Centralized per-frame theme. Applying it before every UI system also repairs
/// context style after browser resize/context recreation. The 44-point minimum
/// interaction height is intentionally suitable for touch targets.
pub fn apply_retro_egui_theme(mut contexts: EguiContexts) {
    apply_retro_style(contexts.ctx_mut());
}

fn apply_retro_style(ctx: &Context) {
    let scale = responsive_scale(ctx);
    let mut style = (*ctx.style()).clone();
    style.visuals.dark_mode = true;
    style.visuals.panel_fill = PANEL_DARK;
    style.visuals.window_fill = PANEL_RAISED;
    style.visuals.extreme_bg_color = Color32::from_rgb(11, 14, 22);
    style.visuals.faint_bg_color = Color32::from_rgb(39, 47, 62);
    style.visuals.override_text_color = Some(Color32::from_rgb(240, 246, 248));
    style.visuals.hyperlink_color = OUTLINE;
    style.visuals.selection.bg_fill = Color32::from_rgb(33, 104, 124);
    style.visuals.selection.stroke = Stroke::new(2.0, ACCENT);
    style.visuals.widgets.inactive.bg_fill = PANEL_RAISED;
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(2.0, Color32::from_rgb(76, 105, 119));
    style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.5, Color32::WHITE);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(39, 82, 98);
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(3.0, OUTLINE);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(75, 64, 39);
    style.visuals.widgets.active.bg_stroke = Stroke::new(3.0, ACCENT);
    style.visuals.widgets.noninteractive.bg_stroke =
        Stroke::new(1.0, Color32::from_rgb(66, 76, 92));
    style.visuals.window_stroke = Stroke::new(3.0, OUTLINE);
    style.visuals.window_rounding = Rounding::same(2.0);
    style.visuals.widgets.inactive.rounding = Rounding::same(2.0);
    style.visuals.widgets.hovered.rounding = Rounding::same(2.0);
    style.visuals.widgets.active.rounding = Rounding::same(2.0);
    style.spacing.item_spacing = vec2(8.0, 8.0);
    style.spacing.button_padding = vec2(16.0, 10.0);
    style.spacing.interact_size = vec2(44.0, 44.0);
    style.spacing.slider_width = 220.0;
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new(22.0 * scale.max(0.75), FontFamily::Monospace),
    );
    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(30.0 * scale.max(0.75), FontFamily::Monospace),
    );
    style.text_styles.insert(
        TextStyle::Body,
        FontId::new(18.0 * scale.max(0.8), FontFamily::Monospace),
    );
    ctx.set_style(style);
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
                    }
                    MenuState::Settings => {
                        next_menu_state.set(MenuState::Main);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

pub fn update_main_menu(
    mut contexts: EguiContexts,
    mut next_game_state: ResMut<NextState<GameState>>,
    mut next_menu_state: ResMut<NextState<MenuState>>,
    mut room: ResMut<MatchmakingRoom>,
) {
    mobile_input::hide();
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
        ui.separator();
        ui.label(RichText::new("HOW TO PLAY").strong().color(Color32::WHITE));
        ui.label("Move: WASD / arrow keys   •   Fire: hold Space or Enter");
        ui.label("Touch: drag on the LEFT to move   •   hold the RIGHT side to fire");
        ui.label("Win rounds by eliminating rivals. First ghost to 3 points wins the match.");
        ui.label("Cyan = speed boost (5 seconds)   •   Gold = one-hit shield   •   Red = trap   •   Purple = Void boundary");
    });

    bevy_egui::egui::CentralPanel::default()
        .frame(
            Frame::none()
                .inner_margin(panel_margin(contexts.ctx_mut()))
                .fill(PANEL_DARK),
        )
        .show(contexts.ctx_mut(), |ui| {
            // set spacing
            ui.style_mut().spacing.indent = 16.0;
            ui.style_mut().spacing.item_spacing = vec2(10.0, 10.0 * scale);
            ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.vertical_centered_justified(|ui| {
                        // set button style
                        if let Some(button_style) =
                            ui.style_mut().text_styles.get_mut(&TextStyle::Button)
                        {
                            *button_style = FontId::new(28.0 * scale, FontFamily::Proportional);
                        }
                        ui.heading("Last Ghost Standing");
                        ui.label("The prominent default: survive a 3–8 ghost arena. Last ghost alive scores; first to 3 wins.");
                        ui.horizontal(|ui| {
                            if ui.selectable_label(room.mode == super::session::GameMode::Deathmatch, "Last Ghost Standing (3–8)").clicked() {
                                room.mode = super::session::GameMode::Deathmatch;
                                room.capacity = room.capacity.clamp(3, 8);
                                room.use_lobby_v2 = true;
                            }
                            if ui.selectable_label(room.mode == super::session::GameMode::Duel, "Dueling Ghosts (2)").clicked() {
                                room.mode = super::session::GameMode::Duel;
                                room.capacity = 2;
                                room.use_lobby_v2 = true;
                            }
                        });
                        if room.mode == super::session::GameMode::Deathmatch {
                            ui.add(Slider::new(&mut room.capacity, 3..=8).text("Ghosts required"));
                        }
                        ui.small(match room.mode {
                            super::session::GameMode::Duel => "Opt-in head-to-head rules • exactly 2 players • first to 3",
                            super::session::GameMode::Deathmatch => "3–8 players • last survivor scores • selected capacity is an immutable roster • first to 3",
                        });
                        if ui.button("▶ Queue Selected Mode").clicked() {
                            room.private_code = None;
                            next_menu_state.set(MenuState::Main);
                            next_game_state.set(GameState::Matchmaking);
                        }
                        if room.mode == super::session::GameMode::Duel {
                            ui.checkbox(&mut room.use_lobby_v2, "Modern lobby (disable for legacy duel fallback)");
                        } else {
                            room.use_lobby_v2 = true;
                        }
                        if ui.button("🔒 Private Match").clicked() {
                            next_menu_state.set(MenuState::DirectConnect);
                        }
                        #[cfg(feature = "sync_test")]
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
    mobile_input::show(MobileInputKind::RoomCode, &code, 16);
    if let Some(value) = mobile_input::value(MobileInputKind::RoomCode) {
        *code = sanitize_room_code(&value);
    }
    let scale = responsive_scale(contexts.ctx_mut());
    CentralPanel::default()
        .frame(
            Frame::none()
                .inner_margin(panel_margin(contexts.ctx_mut()))
                .fill(PANEL_DARK),
        )
        .show(contexts.ctx_mut(), |ui| {
            ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.style_mut().spacing.item_spacing.y = 10.0 * scale;
                    ui.vertical_centered_justified(|ui| {
                        ui.heading("Private Match");
                        ui.label("Enter the same room code on both devices.");
                        if ui.text_edit_singleline(&mut *code).changed() {
                            *code = sanitize_room_code(code.as_str());
                        }
                        if ui
                            .add_enabled(
                                !code.is_empty(),
                                Button::new("Create / Join Private Match"),
                            )
                            .clicked()
                        {
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
    socket: Res<CloudflareSocket>,
    mut next_game: ResMut<NextState<GameState>>,
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
            if ui.button("Exit Lobby").clicked() {
                socket.leave_lobby(false);
                next_game.set(GameState::MainMenu);
            }
        });
}

pub fn update_settings_ui(
    mut contexts: EguiContexts,
    mut next_menu_state: ResMut<NextState<MenuState>>,
    mut audio_config: ResMut<AudioConfig>,
    mut profile: ResMut<PendingPlayerProfile>,
    casual: Res<CasualProfile>,
) {
    mobile_input::show(MobileInputKind::PlayerName, &profile.name, 24);
    if let Some(value) = mobile_input::value(MobileInputKind::PlayerName) {
        let value = PlayerProfile::sanitized_name(&value);
        if !value.is_empty() {
            profile.name = value;
        }
    }
    let scale = responsive_scale(contexts.ctx_mut());
    bevy_egui::egui::CentralPanel::default()
        .frame(
            Frame::none()
                .inner_margin(panel_margin(contexts.ctx_mut()))
                .fill(PANEL_DARK),
        )
        .show(contexts.ctx_mut(), |ui| {
            // set spacing
            ui.style_mut().spacing.indent = 16.0;
            ui.style_mut().spacing.item_spacing = vec2(16.0, 16.0);

            let wide = ui.available_height() < ui.available_width();
            ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.style_mut().spacing.item_spacing.y = 10.0 * scale;
                    ui.vertical_centered_justified(|ui| {
                        // set button style
                        if let Some(button_style) =
                            ui.style_mut().text_styles.get_mut(&TextStyle::Button)
                        {
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
                        ui.style_mut().spacing.slider_width =
                            ui.max_rect().width() - extra_slider_widget_size;

                        ui.heading("Player Settings");

                        let label = ui.label("Player Name: ");
                        if ui
                            .text_edit_singleline(&mut profile.name)
                            .labelled_by(label.id)
                            .changed()
                        {
                            profile.name = PlayerProfile::sanitized_name(&profile.name);
                            if profile.name.is_empty() {
                                profile.name = "Ghost".into();
                            }
                        }
                        ui.horizontal(|ui| {
                            ui.label("Color:");
                            for (id, color) in [
                                Color32::RED,
                                Color32::BLUE,
                                Color32::GREEN,
                                Color32::from_rgb(190, 70, 190),
                            ]
                            .into_iter()
                            .enumerate()
                            {
                                if ui
                                    .selectable_label(
                                        profile.palette_id == id as u8,
                                        RichText::new("●").color(color),
                                    )
                                    .clicked()
                                {
                                    profile.palette_id = id as u8;
                                }
                            }
                        });
                        ui.label(format!(
                            "Casual progress: {} points • {} rounds • {} matches",
                            casual.lifetime_points, casual.rounds_played, casual.matches_played,
                        ));
                        let selected = COSMETICS
                            .get(profile.cosmetic_id as usize)
                            .map(|cosmetic| cosmetic.name)
                            .unwrap_or(COSMETICS[0].name);
                        ComboBox::from_label("Cosmetic")
                            .selected_text(selected)
                            .show_ui(ui, |ui| {
                                for cosmetic in COSMETICS {
                                    let unlocked = casual.is_unlocked(cosmetic.id);
                                    let label = if unlocked {
                                        cosmetic.name.to_owned()
                                    } else {
                                        format!(
                                            "🔒 {} — requires {} lifetime points",
                                            cosmetic.name, cosmetic.required_points
                                        )
                                    };
                                    if ui
                                        .add_enabled(
                                            unlocked,
                                            SelectableLabel::new(
                                                profile.cosmetic_id == cosmetic.id,
                                                label,
                                            ),
                                        )
                                        .clicked()
                                    {
                                        profile.cosmetic_id = cosmetic.id;
                                    }
                                }
                            });
                        ui.small(
                            "Cosmetics are casual local rewards; Classic is always available.",
                        );

                        ui.heading("Volume Settings");

                        if !wide {
                            ui.label("🔊 Master Volume");
                        }
                        ui.add({
                            let mut slider =
                                Slider::new(&mut audio_config.master_volume, 0.0..=100.0)
                                    .show_value(wide)
                                    .trailing_fill(true);
                            if wide {
                                slider = slider.text("🔊 Master Volume");
                            }
                            slider
                        });

                        if !wide {
                            ui.label("🎵 Music Volume");
                        }
                        ui.add({
                            let mut slider =
                                Slider::new(&mut audio_config.music_volume, 0.0..=100.0)
                                    .show_value(wide)
                                    .trailing_fill(true);
                            if wide {
                                slider = slider.text("🎵 Music Volume");
                            }
                            slider
                        });

                        if !wide {
                            ui.label("💥 SFX Volume");
                        }
                        ui.add({
                            let mut slider = Slider::new(&mut audio_config.sfx_volume, 0.0..=100.0)
                                .show_value(wide)
                                .trailing_fill(true);
                            if wide {
                                slider = slider.text("💥 SFX Volume");
                            }
                            slider
                        });

                        // return to main menu
                        if ui.button("Back").clicked() {
                            next_menu_state.set(MenuState::Main);
                        }
                    });
                });
        });
}

fn palette_color(id: u8) -> Color32 {
    [
        Color32::from_rgb(204, 51, 51),
        Color32::from_rgb(38, 64, 204),
        Color32::from_rgb(51, 191, 77),
        Color32::from_rgb(191, 64, 191),
        Color32::from_rgb(242, 166, 38),
        Color32::from_rgb(26, 204, 204),
        Color32::from_rgb(230, 115, 166),
        Color32::from_rgb(166, 191, 51),
    ]
    .get(id as usize)
    .copied()
    .unwrap_or(Color32::WHITE)
}

pub fn update_score_ui(
    mut contexts: EguiContexts,
    scores: Res<Scores>,
    bootstrap: Option<Res<RoundBootstrap>>,
    local: Option<Res<LocalPlayerHandle>>,
) {
    let Some(bootstrap) = bootstrap else {
        return;
    };
    Area::new("score")
        .anchor(Align2::CENTER_TOP, (0., 18.))
        .show(contexts.ctx_mut(), |ui| {
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new(mode_label(bootstrap.mode))
                        .strong()
                        .color(Color32::WHITE),
                );
                ui.horizontal(|ui| {
                    for score in scores.entries() {
                        let profile = bootstrap
                            .profiles
                            .iter()
                            .find(|profile| profile.player_id == score.player_id);
                        let roster = bootstrap
                            .roster
                            .iter()
                            .find(|entry| entry.player_id == score.player_id);
                        let name = profile
                            .map(|profile| profile.name.as_str())
                            .unwrap_or("Ghost");
                        let marker = if roster
                            .map(|entry| {
                                local
                                    .as_ref()
                                    .map(|local| entry.handle == local.0)
                                    .unwrap_or(false)
                            })
                            .unwrap_or(false)
                        {
                            "YOU • "
                        } else {
                            ""
                        };
                        ui.label(
                            RichText::new(format!(
                                "{marker}{name}: {}/{}",
                                score.score, MATCH_POINTS_TO_WIN
                            ))
                            .strong()
                            .color(
                                profile
                                    .map(|profile| palette_color(profile.palette_id))
                                    .unwrap_or(Color32::WHITE),
                            ),
                        );
                    }
                });
            });
        });
}

pub fn update_match_status_ui(
    mut contexts: EguiContexts,
    bootstrap: Option<Res<RoundBootstrap>>,
    local: Option<Res<LocalPlayerHandle>>,
    flow: Res<MatchFlow>,
    rollback: Res<State<RollbackState>>,
    progress: Res<super::RoundProgress>,
    players: Query<(
        &Player,
        Option<&SpeedBoost>,
        Option<&ShieldCharges>,
        Option<&MarkedForDeath>,
    )>,
    mut next_game: ResMut<NextState<GameState>>,
    mut socket: ResMut<CloudflareSocket>,
    mut rematch: ResMut<RematchFlow>,
    mut room: ResMut<MatchmakingRoom>,
    mut toasts: ResMut<super::toasts::Toasts>,
) {
    let (Some(bootstrap), Some(local)) = (bootstrap, local) else {
        return;
    };
    let local_id = bootstrap
        .roster
        .iter()
        .find(|entry| entry.handle == local.0)
        .map(|entry| entry.player_id);
    let local_player = players
        .iter()
        .find(|(player, _, _, _)| player.handle == local.0);

    Area::new("player status")
        .anchor(Align2::LEFT_BOTTOM, (18., -18.))
        .show(contexts.ctx_mut(), |ui| {
            if let Some((_, boost, shield, marked)) = local_player {
                if marked.is_some() {
                    ui.label(
                        RichText::new("ELIMINATED — spectating until the next round")
                            .color(STATUS_DANGER)
                            .strong(),
                    );
                } else {
                    if let Some(boost) = boost {
                        ui.label(
                            RichText::new(format!(
                                "⚡ SPEED  {:.1}s",
                                boost.frames_left as f32 / 60.0
                            ))
                            .color(OUTLINE)
                            .strong(),
                        );
                    }
                    if shield.is_some() {
                        ui.label(
                            RichText::new("◆ SHIELD  blocks 1 hit")
                                .color(ACCENT)
                                .strong(),
                        );
                    }
                }
            } else if local_id
                .map(|id| {
                    progress
                        .eliminated
                        .iter()
                        .any(|entry| entry.player_id == id)
                })
                .unwrap_or(false)
            {
                ui.label(
                    RichText::new("ELIMINATED — spectating until the next round")
                        .color(STATUS_DANGER)
                        .strong(),
                );
            } else if rollback.get() == &RollbackState::InRound {
                ui.label(
                    RichText::new("SPECTATING — waiting for the active ghosts")
                        .color(STATUS_DANGER)
                        .strong(),
                );
            }
        });

    if let MatchFlow::MatchOver { winner } = *flow {
        let winner_name = bootstrap
            .profiles
            .iter()
            .find(|profile| profile.player_id == winner)
            .map(|profile| profile.name.as_str())
            .unwrap_or("Ghost");
        bevy_egui::egui::Window::new("MATCH OVER")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(contexts.ctx_mut(), |ui| {
                ui.heading(if Some(winner) == local_id {
                    "YOU WIN!"
                } else {
                    "MATCH OVER"
                });
                ui.label(format!(
                    "{winner_name} is the first ghost to {MATCH_POINTS_TO_WIN} points."
                ));
                match &*rematch {
                    RematchFlow::Idle => ui.label("Rematch keeps this lobby and roster. Every opponent must accept within 10 seconds."),
                    RematchFlow::Pending { deadline_ms, accepted, required, .. } => {
                        let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|v| v.as_millis() as u64).unwrap_or(0);
                        let seconds = deadline_ms.saturating_sub(now).saturating_add(999) / 1000;
                        ui.label(format!("Rematch requested — {accepted}/{required} accepted • {seconds}s. Accept or deny."))
                    }
                };
                ui.horizontal_wrapped(|ui| {
                    match rematch.clone() {
                        RematchFlow::Idle => {
                            if ui.button("Rematch (Same Lobby)").clicked() {
                                let generation = socket.match_generation().unwrap_or(0).saturating_add(1);
                                let nonce = format!("{:032x}", bootstrap.match_id.0 ^ generation as u128 ^ local_id.unwrap_or_default().0);
                                if socket.request_rematch(generation, &nonce) {
                                    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|v| v.as_millis() as u64).unwrap_or(0);
                                    *rematch = RematchFlow::Pending { generation, nonce, deadline_ms: now + 10_000, accepted: 1, required: bootstrap.roster.len() as u8 };
                                } else {
                                    toasts.error("Could not send rematch request; returning to menu.".into());
                                    next_game.set(GameState::MainMenu);
                                }
                            }
                        }
                        RematchFlow::Pending { generation, nonce, .. } => {
                            if ui.button("Accept Rematch").clicked() && !socket.respond_rematch(generation, &nonce, true) {
                                toasts.error("Could not send rematch response; returning to menu.".into());
                                next_game.set(GameState::MainMenu);
                            }
                            if ui.button("Deny").clicked() && !socket.respond_rematch(generation, &nonce, false) {
                                toasts.error("Could not send rematch denial; returning to menu.".into());
                                next_game.set(GameState::MainMenu);
                            }
                        }
                    }
                    if ui.button("Re-Queue (General Queue)").clicked() {
                        socket.leave_lobby(true);
                        room.private_code = None;
                        room.mode = super::session::GameMode::Deathmatch;
                        room.capacity = 8;
                        socket.disconnect();
                        *rematch = RematchFlow::Idle;
                        next_game.set(GameState::Matchmaking);
                    }
                    if ui.button("Main Menu").clicked() {
                        socket.leave_lobby(false);
                        next_game.set(GameState::MainMenu);
                    }
                });
            });
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;

    #[test]
    fn responsive_layout_stays_compact_on_mobile() {
        for size in [vec2(390.0, 844.0), vec2(844.0, 390.0)] {
            let ctx = Context::default();
            ctx.set_pixels_per_point(1.0);
            ctx.begin_frame(RawInput {
                screen_rect: Some(bevy_egui::egui::Rect::from_min_size(Pos2::ZERO, size)),
                ..default()
            });
            let margin = panel_margin(&ctx);
            assert!(margin.left <= 40.0 && margin.top <= 32.0);
            assert!((0.6..=1.0).contains(&responsive_scale(&ctx)));
            apply_retro_style(&ctx);
            let style = ctx.style();
            assert!(style.spacing.interact_size.x >= 44.0);
            assert!(style.spacing.interact_size.y >= 44.0);
            assert_eq!(style.visuals.panel_fill, PANEL_DARK);
            assert_eq!(style.visuals.widgets.hovered.bg_stroke.color, OUTLINE);
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
                RichText::new("Game by Connor Postma 2023")
                    .color(Color32::GRAY)
                    .font(FontId::monospace(24.0)),
            );
            ui.label(
                RichText::new("Waiting for opponent to join...")
                    .color(Color32::WHITE)
                    .font(FontId::proportional(48.0)),
            );
        });
}

/// Practice information is deliberately separate from the matchmaking status
/// panel above: the network wait remains visible while the player trains.
pub fn update_practice_ui(
    mut contexts: EguiContexts,
    score: Res<PracticeScore>,
    cooldown: Res<PracticeCooldown>,
) {
    Area::new("practice HUD")
        .anchor(Align2::CENTER_BOTTOM, (0.0, -18.0))
        .show(contexts.ctx_mut(), |ui| {
            Frame::none()
                .fill(Color32::from_rgba_unmultiplied(20, 24, 34, 225))
                .stroke(Stroke::new(2.0, OUTLINE))
                .inner_margin(Margin::symmetric(14.0, 8.0))
                .show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("TARGET PRACTICE").strong().color(ACCENT));
                        ui.label(format!(
                            "Score {:04}  •  Streak {}  •  Best {}",
                            score.score, score.streak, score.best_streak
                        ));
                        ui.small("Move: WASD / arrows  •  Fire: Space / Enter");
                        ui.small("Touch: drag LEFT to move  •  hold RIGHT to fire");
                        if cooldown.remaining > 0.0 {
                            ui.small(format!("Blaster cooling {:.1}s", cooldown.remaining));
                        } else {
                            ui.small("Blaster ready");
                        }
                    });
                });
        });
}

pub fn update_respawn_ui(mut contexts: EguiContexts, flow: Res<MatchFlow>) {
    if matches!(*flow, MatchFlow::MatchOver { .. }) {
        return;
    }
    Area::new("matchmaking info")
        .anchor(Align2::CENTER_CENTER, (0., 25.))
        .show(contexts.ctx_mut(), |ui| {
            ui.label(
                RichText::new("SCORE!\nRespawning...")
                    .color(Color32::WHITE)
                    .font(FontId::proportional(32.0)),
            );
            ui.spinner();
        });
}
