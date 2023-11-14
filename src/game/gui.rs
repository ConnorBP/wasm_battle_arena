use bevy::prelude::*;
use bevy_egui::{egui::*, EguiContexts};
use bevy_kira_audio::Audio;

use super::{Scores, GameState, assets::sounds::AudioConfig};

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
            GameState::InGame => {
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
        .inner_margin(Margin::symmetric(100., 200.))
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
            next_menu_state.set(MenuState::Main);
            next_game_state.set(GameState::Matchmaking);
        }
        // if ui.button("Direct Connect").clicked() {
        //     next_menu_state.set(MenuState::DirectConnect);
        // }
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

pub fn update_settings_ui(
    mut contexts: EguiContexts,
    mut next_menu_state: ResMut<NextState<MenuState>>,
    mut audio_config: ResMut<AudioConfig>,
    mut test_name: Local<String>,
) {
    bevy_egui::egui::CentralPanel::default()
    .frame(
        Frame::none()
        .inner_margin(Margin::symmetric(100., 200.))
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

            // justify the sliders (- 200 for extra display value and text size)
            ui.style_mut().spacing.slider_width = ui.max_rect().width() - 200.;

            ui.heading("Player Settings");

            let label = ui.label("Player Name: ");
            ui.text_edit_singleline(&mut *test_name).labelled_by(label.id);


            ui.heading("Volume Settings");
            
            ui.add(
                Slider::new(&mut audio_config.master_volume, 0.0..=100.0)
                .text("Master Volume")
                .trailing_fill(true)
            );

            ui.spacing();
            
            ui.add(
                Slider::new(&mut audio_config.music_volume, 0.0..=100.0)
                .text("Music Volume")
                .trailing_fill(true)
            );
            ui.add(
                Slider::new(&mut audio_config.sfx_volume, 0.0..=100.0)
                .text("SFX Volume")
                .trailing_fill(true)
            );

            // return to main menu
            if ui.button("Back").clicked() {
                next_menu_state.set(MenuState::Main);
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


  // Helper function to center arbitrary widgets. It works by measuring the width of the widgets after rendering, and
  // then using that offset on the next frame.
  fn centerer(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
      let id = ui.id().with("_centerer");
      let last_width: Option<f32> = ui.memory_mut(|mem| mem.data.get_temp(id));
      if let Some(last_width) = last_width {
        ui.add_space((ui.available_width() - last_width) / 2.0);
      }
      let res = ui
        .scope(|ui| {
          add_contents(ui);
        })
        .response;
      let width = res.rect.width();
      ui.memory_mut(|mem| mem.data.insert_temp(id, width));

      // Repaint if width changed
      match last_width {
        None => ui.ctx().request_repaint(),
        Some(last_width) if last_width != width => ui.ctx().request_repaint(),
        Some(_) => {}
      }
    });
  }