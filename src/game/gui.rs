use bevy::prelude::*;
use bevy_egui::{egui::*, EguiContexts};
use egui_plot::{Plot, Legend, Polygon};

use super::{Scores, RoundEndTimer};

// for loading circle
use std::f64::consts::TAU;
const FULL_CIRCLE_VERTICES: f64 = 240.0;
const RADIUS: f64 = 1.0;

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
                RichText::new(format!("Waiting for players"))
                    .color(Color32::WHITE)
                    .font(FontId::proportional(72.0)),
            );
        });
}

pub fn update_respawn_ui(mut contexts: EguiContexts, timer: Res<RoundEndTimer>) {

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

