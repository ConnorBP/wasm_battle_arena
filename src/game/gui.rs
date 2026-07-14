use crate::mobile_input::{self, MobileInputKind};
use bevy::prelude::*;
use bevy_egui::{
    egui::{self, *},
    EguiContexts,
};
use instant::SystemTime;

use super::{
    assets::sounds::AudioConfig,
    components::{MarkedForDeath, Player, ShieldCharges, SpeedBoost},
    networking::{sanitize_room_code, LocalPlayerHandle, MatchmakingRoom},
    practice::{PracticeCooldown, PracticeScore},
    progression::{CasualProfile, COSMETICS},
    session::{mode_label, MatchPreference, PlayerProfile, RoundBootstrap, MATCH_POINTS_TO_WIN},
    GameState, MatchFlow, PendingPlayerProfile, RematchFlow, RollbackState, Scores,
};
use crate::cloudflare_net::{CloudflareSocket, QueueStatus};

#[derive(States, Clone, Eq, PartialEq, Debug, Hash, Default)]
pub enum MenuState {
    #[default]
    Main,

    DirectConnect,
    Pause,
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
const MOBILE_EDGE_MARGIN: f32 = 12.0;
const NARROW_LAYOUT_WIDTH: f32 = 560.0;

fn safe_screen_rect_for(screen: egui::Rect) -> egui::Rect {
    let inset_x = MOBILE_EDGE_MARGIN.min(screen.width() * 0.5);
    let inset_y = MOBILE_EDGE_MARGIN.min(screen.height() * 0.5);
    egui::Rect::from_min_max(
        screen.min + vec2(inset_x, inset_y),
        screen.max - vec2(inset_x, inset_y),
    )
}

fn safe_screen_rect(ctx: &Context) -> egui::Rect {
    safe_screen_rect_for(ctx.screen_rect())
}

#[cfg_attr(not(test), allow(dead_code))]
fn is_narrow(width: f32) -> bool {
    width < NARROW_LAYOUT_WIDTH
}

fn panel_margin(ctx: &Context) -> Margin {
    let size = ctx.screen_rect().size();
    let scale = responsive_scale(ctx);
    let horizontal = ((size.x * 0.06).clamp(12.0, 40.0) * scale).max(MOBILE_EDGE_MARGIN);
    let vertical = ((size.y * 0.04).clamp(12.0, 32.0) * scale).max(MOBILE_EDGE_MARGIN);
    Margin::symmetric(horizontal, vertical)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PauseAction {
    Resume,
    Settings,
    ExitLobby,
    MainMenu,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PauseActionEffect {
    menu: MenuState,
    game: Option<GameState>,
    notify_worker_leave: bool,
    requeue: bool,
}

fn pause_action_effect(action: PauseAction) -> PauseActionEffect {
    match action {
        PauseAction::Resume => PauseActionEffect {
            menu: MenuState::Main,
            game: None,
            notify_worker_leave: false,
            requeue: false,
        },
        PauseAction::Settings => PauseActionEffect {
            menu: MenuState::Settings,
            game: None,
            notify_worker_leave: false,
            requeue: false,
        },
        PauseAction::ExitLobby | PauseAction::MainMenu => PauseActionEffect {
            menu: MenuState::Main,
            game: Some(GameState::MainMenu),
            notify_worker_leave: true,
            requeue: false,
        },
    }
}

fn escape_destination(game: &GameState, menu: &MenuState) -> Option<MenuState> {
    match (game, menu) {
        (GameState::InGame, MenuState::Main) => Some(MenuState::Pause),
        (GameState::InGame, MenuState::Pause) => Some(MenuState::Main),
        (GameState::InGame, MenuState::Settings) => Some(MenuState::Pause),
        _ => None,
    }
}

fn settings_back_destination(game: &GameState) -> MenuState {
    if game == &GameState::InGame {
        MenuState::Pause
    } else {
        MenuState::Main
    }
}

/// Pause is deliberately UI-only. In-game network/GGRS/input scheduling is
/// keyed solely to GameState::InGame and must continue for every MenuState.
#[cfg_attr(not(test), allow(dead_code))]
fn in_game_runtime_scheduled(game: &GameState, _menu: &MenuState) -> bool {
    game == &GameState::InGame
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
    style.visuals.selection.stroke = Stroke::new(2.0_f32, ACCENT);
    style.visuals.widgets.inactive.bg_fill = PANEL_RAISED;
    style.visuals.widgets.inactive.bg_stroke =
        Stroke::new(2.0_f32, Color32::from_rgb(76, 105, 119));
    style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.5_f32, Color32::WHITE);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(39, 82, 98);
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(3.0_f32, OUTLINE);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(75, 64, 39);
    style.visuals.widgets.active.bg_stroke = Stroke::new(3.0_f32, ACCENT);
    style.visuals.widgets.noninteractive.bg_stroke =
        Stroke::new(1.0_f32, Color32::from_rgb(66, 76, 92));
    style.visuals.window_stroke = Stroke::new(3.0_f32, OUTLINE);
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
        if let Some(destination) =
            escape_destination(current_game_state.get(), current_menu_state.get())
        {
            next_menu_state.set(destination);
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
    let screen = contexts.ctx_mut().screen_rect();
    let safe = safe_screen_rect_for(screen);
    bevy_egui::egui::CentralPanel::default()
        .frame(
            Frame::none()
                .outer_margin(Margin {
                    left: safe.left(),
                    right: screen.right() - safe.right(),
                    top: safe.top(),
                    bottom: screen.bottom() - safe.bottom(),
                })
                .inner_margin(Margin::same(0.0))
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
                        ui.label(
                            RichText::new(format!("GHOSTIES {}", env!("CARGO_PKG_VERSION")))
                                .color(Color32::LIGHT_BLUE)
                                .font(FontId::proportional(42.0 * scale)),
                        );
                        ui.label("Game by Connor Postma • Music by Warren Postma");
                        ui.separator();
                        ui.label(RichText::new("HOW TO PLAY").strong());
                        ui.label("Move: WASD / arrows • Fire: Space / Enter");
                        ui.label("Touch: drag LEFT to move • hold RIGHT to fire");
                        ui.label("Eliminate rivals; first ghost to 3 points wins.");
                        ui.separator();
                        // set button style
                        if let Some(button_style) =
                            ui.style_mut().text_styles.get_mut(&TextStyle::Button)
                        {
                            *button_style = FontId::new(28.0 * scale, FontFamily::Proportional);
                        }
                        ui.heading("Public Matchmaking");
                        ui.label("Any is recommended: find the quickest fair Duel or 3–8 ghost Last Ghost Standing match.");
                        ui.horizontal_wrapped(|ui| {
                            if ui.selectable_label(room.preference == MatchPreference::Any, "★ Any (Recommended)").clicked() {
                                room.preference = MatchPreference::Any;
                            }
                            if ui.selectable_label(room.preference == MatchPreference::Duel, "Dueling Ghosts").clicked() {
                                room.preference = MatchPreference::Duel;
                            }
                            if ui.selectable_label(room.preference == MatchPreference::LastGhostStanding, "Last Ghost Standing").clicked() {
                                room.preference = MatchPreference::LastGhostStanding;
                            }
                        });
                        ui.small(match room.preference {
                            MatchPreference::Any => "Recommended • coordinator may assign Duel or Last Ghost Standing",
                            MatchPreference::Duel => "Exactly 2 players • first to 3",
                            MatchPreference::LastGhostStanding => "3–8 players • last survivor scores • first to 3",
                        });
                        if ui.button("▶ Find Public Match").clicked() {
                            room.private_code = None;
                            next_menu_state.set(MenuState::Main);
                            next_game_state.set(GameState::Matchmaking);
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
    let margin = panel_margin(contexts.ctx_mut());
    CentralPanel::default()
        .frame(
            Frame::none()
                .outer_margin(margin)
                .inner_margin(Margin::same(0.0))
                .fill(PANEL_DARK),
        )
        .show(contexts.ctx_mut(), |ui| {
            ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.style_mut().spacing.item_spacing.y = 10.0 * scale;
                    ui.vertical_centered_justified(|ui| {
                        ui.heading("Private Match");
                        ui.label("Enter the same room code and choose the same exact mode on every device. Private rooms connect directly with protocol 3.");
                        ui.horizontal_wrapped(|ui| {
                            if ui.selectable_label(room.private_mode == super::session::GameMode::Duel, "Dueling Ghosts (2)").clicked() {
                                room.private_mode = super::session::GameMode::Duel;
                                room.private_capacity = 2;
                            }
                            if ui.selectable_label(room.private_mode == super::session::GameMode::Deathmatch, "Last Ghost Standing (Exact 3–8)").clicked() {
                                room.private_mode = super::session::GameMode::Deathmatch;
                                room.private_capacity = room.private_capacity.clamp(3, 8);
                            }
                        });
                        if room.private_mode == super::session::GameMode::Deathmatch {
                            ui.label(RichText::new("Choose the exact private LGS roster size").strong().color(ACCENT));
                            ui.add(Slider::new(&mut room.private_capacity, 3..=8).text("Exact ghosts (3–8)"));
                        }
                        if ui.text_edit_singleline(&mut *code).changed() {
                            *code = sanitize_room_code(code.as_str());
                        }
                        if ui
                            .add_enabled_ui(!code.is_empty(), |ui| {
                                ui.add_sized(
                                    vec2(ui.available_width(), 44.0),
                                    Button::new("Create / Join Private Match"),
                                )
                            })
                            .inner
                            .clicked()
                        {
                            room.private_code = Some(code.clone());
                            next_menu_state.set(MenuState::Main);
                            next_game_state.set(GameState::Matchmaking);
                        }
                        if ui
                            .add_sized(vec2(ui.available_width(), 44.0), Button::new("Back"))
                            .clicked()
                        {
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
    let safe = safe_screen_rect(contexts.ctx_mut());
    Area::new("controls menu")
        .fixed_pos(safe.min)
        .show(contexts.ctx_mut(), |ui| {
            if ui
                .add_sized(
                    vec2(112.0_f32.min(safe.width()), 44.0),
                    Button::new("☰ MENU"),
                )
                .clicked()
            {
                next_menu_state.set(MenuState::Pause);
            }
        });
}

pub fn update_pause_ui(
    mut contexts: EguiContexts,
    mut next_menu_state: ResMut<NextState<MenuState>>,
    mut next_game_state: ResMut<NextState<GameState>>,
    socket: Res<CloudflareSocket>,
) {
    mobile_input::hide();
    let safe = safe_screen_rect(contexts.ctx_mut());
    egui::Window::new("GAME MENU")
        .id(Id::new("pause overlay"))
        .collapsible(false)
        .resizable(false)
        .title_bar(true)
        .fixed_pos(safe.min)
        .default_size(safe.size())
        .show(contexts.ctx_mut(), |ui| {
            ui.set_max_width(safe.width());
            ui.set_max_height(safe.height());
            ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.vertical_centered_justified(|ui| {
                        ui.heading("Game Menu");
                        ui.label(
                            RichText::new(
                                "Multiplayer continues while this overlay is open. You are not paused.",
                            )
                            .color(ACCENT)
                            .strong(),
                        );
                        ui.label("Network play and local controls remain live behind this menu.");
                        ui.separator();
                        for (action, label) in [
                            (PauseAction::Resume, "Resume"),
                            (PauseAction::Settings, "Settings"),
                            (PauseAction::ExitLobby, "Exit Lobby"),
                            (PauseAction::MainMenu, "Main Menu"),
                        ] {
                            if ui.add_sized(vec2(ui.available_width(), 44.0), Button::new(label)).clicked() {
                                let effect = pause_action_effect(action);
                                if effect.notify_worker_leave {
                                    socket.leave_lobby(effect.requeue);
                                }
                                next_menu_state.set(effect.menu);
                                if let Some(game) = effect.game {
                                    next_game_state.set(game);
                                }
                            }
                        }
                    });
                });
        });
}

pub fn update_settings_ui(
    mut contexts: EguiContexts,
    game_state: Res<State<GameState>>,
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
    let margin = panel_margin(contexts.ctx_mut());
    bevy_egui::egui::CentralPanel::default()
        .frame(
            Frame::none()
                .outer_margin(margin)
                .inner_margin(Margin::same(0.0))
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
                            (ui.available_width() - extra_slider_widget_size).clamp(44.0, 420.0);

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
                        ui.horizontal_wrapped(|ui| {
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

                        if ui
                            .add_sized(vec2(ui.available_width(), 44.0), Button::new("Back"))
                            .clicked()
                        {
                            next_menu_state.set(settings_back_destination(game_state.get()));
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
    let safe = safe_screen_rect(contexts.ctx_mut());
    let menu_reserve = if safe.width() >= 360.0 { 124.0 } else { 0.0 };
    let score_top = safe.top() + if menu_reserve == 0.0 { 52.0 } else { 0.0 };
    Area::new("score")
        .fixed_pos(pos2(safe.left() + menu_reserve, score_top))
        .show(contexts.ctx_mut(), |ui| {
            ui.set_width((safe.width() - menu_reserve).max(0.0));
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new(mode_label(bootstrap.mode))
                        .strong()
                        .color(Color32::WHITE),
                );
                ui.horizontal_wrapped(|ui| {
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

    let safe = safe_screen_rect(contexts.ctx_mut());
    Area::new("player status")
        .fixed_pos(pos2(safe.left(), (safe.bottom() - 88.0).max(safe.top())))
        .show(contexts.ctx_mut(), |ui| {
            ui.set_max_width(safe.width());
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
        mobile_input::hide();
        bevy_egui::egui::Window::new("MATCH OVER")
            .collapsible(false)
            .resizable(false)
            .fixed_pos(safe.min)
            .default_size(safe.size())
            .show(contexts.ctx_mut(), |ui| {
                ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
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
                        let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|v| v.as_millis() as u64).unwrap_or(0);
                        let seconds = deadline_ms.saturating_sub(now).saturating_add(999) / 1000;
                        ui.label(format!("Rematch requested — {accepted}/{required} accepted • {seconds}s. Accept or deny."))
                    }
                };
                let narrow_actions = is_narrow(ui.available_width());
                let action_width = if narrow_actions {
                    ui.available_width()
                } else {
                    220.0_f32.min(ui.available_width())
                };
                ui.horizontal_wrapped(|ui| {
                    match rematch.clone() {
                        RematchFlow::Idle => {
                            if ui.add_sized(vec2(action_width, 44.0), Button::new("Rematch (Same Lobby)")).clicked() {
                                let generation = socket.match_generation().unwrap_or(0).saturating_add(1);
                                let nonce = format!("{:032x}", bootstrap.match_id.0 ^ generation as u128 ^ local_id.unwrap_or_default().0);
                                if socket.request_rematch(generation, &nonce) {
                                    let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|v| v.as_millis() as u64).unwrap_or(0);
                                    *rematch = RematchFlow::Pending { generation, nonce, deadline_ms: now + 10_000, accepted: 1, required: bootstrap.roster.len() as u8 };
                                } else {
                                    toasts.error("Could not send rematch request; returning to menu.".into());
                                    next_game.set(GameState::MainMenu);
                                }
                            }
                        }
                        RematchFlow::Pending { generation, nonce, .. } => {
                            if ui.add_sized(vec2(action_width, 44.0), Button::new("Accept Rematch")).clicked() && !socket.respond_rematch(generation, &nonce, true) {
                                toasts.error("Could not send rematch response; returning to menu.".into());
                                next_game.set(GameState::MainMenu);
                            }
                            if ui.add_sized(vec2(action_width, 44.0), Button::new("Deny")).clicked() && !socket.respond_rematch(generation, &nonce, false) {
                                toasts.error("Could not send rematch denial; returning to menu.".into());
                                next_game.set(GameState::MainMenu);
                            }
                        }
                    }
                    if ui.add_sized(vec2(action_width, 44.0), Button::new("Re-Queue (General Queue)")).clicked() {
                        socket.leave_lobby(true);
                        room.private_code = None;
                        room.preference = MatchPreference::Any;
                        socket.disconnect();
                        *rematch = RematchFlow::Idle;
                        next_game.set(GameState::Matchmaking);
                    }
                    if ui.add_sized(vec2(action_width, 44.0), Button::new("Main Menu")).clicked() {
                        socket.leave_lobby(false);
                        next_game.set(GameState::MainMenu);
                    }
                });
                });
            });
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;

    #[test]
    fn pause_and_escape_transitions_are_game_state_aware() {
        assert_eq!(
            escape_destination(&GameState::InGame, &MenuState::Main),
            Some(MenuState::Pause)
        );
        assert_eq!(
            escape_destination(&GameState::InGame, &MenuState::Pause),
            Some(MenuState::Main)
        );
        assert_eq!(
            escape_destination(&GameState::InGame, &MenuState::Settings),
            Some(MenuState::Pause)
        );
        assert_eq!(
            escape_destination(&GameState::MainMenu, &MenuState::Settings),
            None
        );
        assert_eq!(
            settings_back_destination(&GameState::InGame),
            MenuState::Pause
        );
        assert_eq!(
            settings_back_destination(&GameState::MainMenu),
            MenuState::Main
        );
    }

    #[test]
    fn pause_actions_leave_safely_without_requeue() {
        assert_eq!(
            pause_action_effect(PauseAction::Resume).menu,
            MenuState::Main
        );
        assert_eq!(
            pause_action_effect(PauseAction::Settings).menu,
            MenuState::Settings
        );
        for action in [PauseAction::ExitLobby, PauseAction::MainMenu] {
            let effect = pause_action_effect(action);
            assert_eq!(effect.game, Some(GameState::MainMenu));
            assert!(effect.notify_worker_leave);
            assert!(!effect.requeue);
            assert_eq!(effect.menu, MenuState::Main);
        }
    }

    #[test]
    fn pause_does_not_stop_ingame_network_or_input_scheduling() {
        assert!(in_game_runtime_scheduled(
            &GameState::InGame,
            &MenuState::Main
        ));
        assert!(in_game_runtime_scheduled(
            &GameState::InGame,
            &MenuState::Pause
        ));
        assert!(in_game_runtime_scheduled(
            &GameState::InGame,
            &MenuState::Settings
        ));
        assert!(!in_game_runtime_scheduled(
            &GameState::MainMenu,
            &MenuState::Pause
        ));
    }

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
            assert!(margin.left >= MOBILE_EDGE_MARGIN && margin.top >= MOBILE_EDGE_MARGIN);
            assert!(margin.left <= 40.0 && margin.top <= 32.0);
            assert!(is_narrow(size.x) == (size.x < NARROW_LAYOUT_WIDTH));
            let safe = safe_screen_rect(&ctx);
            assert!(safe.left() >= ctx.screen_rect().left());
            assert!(safe.top() >= ctx.screen_rect().top());
            assert!(safe.right() <= ctx.screen_rect().right());
            assert!(safe.bottom() <= ctx.screen_rect().bottom());
            assert!(safe.left() >= MOBILE_EDGE_MARGIN);
            assert!(safe.top() >= MOBILE_EDGE_MARGIN);
            assert!(safe.right() <= size.x - MOBILE_EDGE_MARGIN);
            assert!(safe.bottom() <= size.y - MOBILE_EDGE_MARGIN);
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

    #[test]
    fn safe_bounds_remain_valid_at_extreme_narrow_widths() {
        for size in [vec2(240.0, 320.0), vec2(320.0, 240.0), vec2(24.0, 44.0)] {
            let screen = egui::Rect::from_min_size(Pos2::ZERO, size);
            let safe = safe_screen_rect_for(screen);
            assert!(safe.left() >= screen.left());
            assert!(safe.top() >= screen.top());
            assert!(safe.right() <= screen.right());
            assert!(safe.bottom() <= screen.bottom());
            assert!(safe.width() >= 0.0 && safe.height() >= 0.0);
            assert!(is_narrow(size.x));
        }
    }
}

pub fn update_matchmaking_ui(
    mut contexts: EguiContexts,
    mut next_game_state: ResMut<NextState<GameState>>,
    socket: Res<CloudflareSocket>,
) {
    let safe = safe_screen_rect(contexts.ctx_mut());
    let scale = responsive_scale(contexts.ctx_mut());
    Area::new("matchmaking info")
        .fixed_pos(safe.min)
        .show(contexts.ctx_mut(), |ui| {
            ui.set_width(safe.width());
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new(format!("GHOSTIES {}", env!("CARGO_PKG_VERSION")))
                        .color(Color32::LIGHT_BLUE)
                        .font(FontId::proportional(42.0 * scale)),
                );
                ui.label(RichText::new("Game by Connor Postma 2023").color(Color32::GRAY));
                let queue_status = socket.queue_status();
                let status = match queue_status {
                    Some(QueueStatus::Searching) => "Searching the public queue…".to_owned(),
                    Some(QueueStatus::HoldingForThird) => {
                        "Holding briefly for a third ghost…".to_owned()
                    }
                    Some(QueueStatus::Staging { count, votes, votes_required, deadline_ms, .. }) => {
                        let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)
                            .map(|value| value.as_millis() as u64).unwrap_or(0);
                        let seconds = deadline_ms.saturating_sub(now).saturating_add(999) / 1000;
                        format!("LAST GHOST STANDING ASSEMBLED\n{count} ghosts • {votes}/{votes_required} start votes • auto-start in {seconds}s")
                    }
                    Some(QueueStatus::Assigned) => {
                        "Assigned — joining the secure lobby…".to_owned()
                    }
                    None => "Connecting to matchmaking…".to_owned(),
                };
                ui.label(
                    RichText::new(status)
                        .color(Color32::WHITE)
                        .font(FontId::proportional(28.0 * scale)),
                );
                if let Some(QueueStatus::Staging { voted, .. }) = queue_status {
                    let label = if voted { "Withdraw Vote" } else { "Vote to Start" };
                    if ui.add_sized(vec2(220.0_f32.min(safe.width()), 44.0), Button::new(label)).clicked() {
                        if voted { socket.withdraw_start_vote(); } else { socket.vote_start(); }
                    }
                }
                if ui
                    .add_sized(
                        vec2(220.0_f32.min(safe.width()), 44.0),
                        Button::new("Cancel"),
                    )
                    .clicked()
                {
                    next_game_state.set(GameState::MainMenu);
                }
            });
        });
}

/// Practice information is deliberately separate from the matchmaking status
/// panel above: the network wait remains visible while the player trains.
pub fn update_practice_ui(
    mut contexts: EguiContexts,
    score: Res<PracticeScore>,
    cooldown: Res<PracticeCooldown>,
) {
    let safe = safe_screen_rect(contexts.ctx_mut());
    Area::new("practice HUD")
        .fixed_pos(pos2(safe.left(), (safe.bottom() - 150.0).max(safe.top())))
        .show(contexts.ctx_mut(), |ui| {
            ui.set_width(safe.width());
            Frame::none()
                .fill(Color32::from_rgba_unmultiplied(20, 24, 34, 225))
                .stroke(Stroke::new(2.0_f32, OUTLINE))
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
    let safe = safe_screen_rect(contexts.ctx_mut());
    Area::new("respawn info")
        .fixed_pos(pos2(safe.left(), safe.center().y - 44.0))
        .show(contexts.ctx_mut(), |ui| {
            ui.set_width(safe.width());
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("SCORE!\nRespawning...")
                        .color(Color32::WHITE)
                        .font(FontId::proportional(32.0)),
                );
                ui.spinner();
            });
        });
}
