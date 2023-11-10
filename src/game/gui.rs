use bevy::prelude::*;
use bevy_egui::{egui::*, EguiContexts};

use super::{Scores, GameState};

#[derive(States, Clone, Eq, PartialEq, Debug, Hash, Default)]
pub enum MenuState {
    #[default]
    Main,
    InGame,
    DirectConnect,
    // sub menu of DirectConnect
    HostLobby,
    // sub menu of DirectConnect
    JoinLobby,
    Settings,
    #[cfg(feature="sync_test")]
    SyncTest
}

pub fn update_main_menu(
    mut contexts: EguiContexts,
    mut next_game_state: ResMut<NextState<GameState>>,
    mut next_menu_state: ResMut<NextState<MenuState>>,
) {


    TopBottomPanel::top("main menu top")
    .show(contexts.ctx_mut(), |ui| {
        ui.label(
            RichText::new(format!("GHOSTIES {}", env!("CARGO_PKG_VERSION")))
                .color(Color32::LIGHT_BLUE)
                .font(FontId::proportional(68.0)),
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
        .inner_margin(Margin::symmetric(100., 10.))
        .fill(Color32::from_rgb(66, 69, 73))
    )
    .show(contexts.ctx_mut(), |ui| {
        // set spacing
        ui.style_mut().spacing.indent = 16.0;
        ui.style_mut().spacing.item_spacing = vec2(16.0, 16.0);



        ui.vertical_centered_justified(|ui| {
            // set button style
        if let Some(button_style) = ui.style_mut().text_styles.get_mut(&TextStyle::Button) {
            *button_style = FontId::new(24.0, FontFamily::Proportional);
        }
        if ui.button("Start Matchmaking").clicked() {
            next_game_state.set(GameState::Matchmaking);
            next_menu_state.set(MenuState::InGame);
        }
        if ui.button("Direct Connect").clicked() {
            next_menu_state.set(MenuState::DirectConnect);
        }
        #[cfg(feature="sync_test")]
        if ui.button("SyncTest").clicked() {
            next_menu_state.set(MenuState::SyncTest);
        }
        if ui.button("Settings").clicked() {
            next_menu_state.set(MenuState::Settings);
        }
        });
    });
}


pub fn update_score_ui(mut contexts: EguiContexts, scores: Res<Scores>) {
    let Scores(p1_score, p2_score) = *scores;

    Area::new("score")
    .anchor(Align2::CENTER_TOP, (0., 25.))
    .show(contexts.ctx_mut(), |ui| {
        ui.label(
            RichText::new(format!("{p1_score} : {p2_score}"))
                .color(Color32::WHITE)
                .font(FontId::proportional(72.0)),
        );
    });
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

