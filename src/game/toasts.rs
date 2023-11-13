use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui::WidgetText};
use egui_toast::Toast;

#[derive(Resource)]
pub struct Toasts(pub(crate) egui_toast::Toasts);

impl Toasts {
    pub fn error(&mut self, text: WidgetText) {
        self.0.add(Toast{
            text,
            kind: egui_toast::ToastKind::Error,
            options: egui_toast::ToastOptions::default()
                .duration_in_seconds(5.0)
                .show_progress(true)
        });
    }
}

impl Default for Toasts {
    fn default() -> Self {
        Self(egui_toast::Toasts::new())
    }
}

/// Runs every frame to display currently active toasts on our gui
pub fn display_toasts(
    mut contexts: EguiContexts, 
    mut toasts: ResMut<Toasts>,

) {
    toasts.0.show(contexts.ctx_mut())
}