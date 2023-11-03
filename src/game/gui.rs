use bevy::prelude::*;
use bevy_egui::{egui::*, EguiContexts};

use super::Scores;

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
                RichText::new(format!("GHOSTIES"))
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

