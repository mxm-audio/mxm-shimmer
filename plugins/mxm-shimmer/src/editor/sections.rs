//! The Space, Ascent and Loop cards, in signal-flow order.

use std::collections::HashMap;

use egui::{Rect, Ui};
use mxm_ui::control::Size;
use mxm_ui::space::SPACE_2;
use mxm_ui::theme::Tokens;
use mxm_ui::tree::{self, Height, Kind, Node, Share};
use mxm_ui::visual::{
    AXIS_STROKE, CANVAS_RADIUS, EMPHASIS_STROKE, INNER_GUTTER, TALL_PLOT_HEIGHT, TRACE_STROKE,
};
use nice_plug::prelude::ParamSetter;

use super::binding::{Bound, segmented, toggle_labelled};
use crate::params::MxmShimmerParams;

const KNOB: Size = Size::Standard;
/// The narrowest the bloom display is drawn; it fills its card's width at `TALL_PLOT_HEIGHT`.
const BLOOM_MIN_WIDTH: f32 = 200.0;

pub fn all_parameters(params: &MxmShimmerParams) -> Vec<Bound<'_>> {
    vec![
        Bound::new(
            "predelay",
            &params.pre_delay,
            "Delays the start of the reverb; the dry sound stays on time.",
        ),
        Bound::new(
            "predelaysync",
            &params.pre_delay_sync,
            super::binding::SYNC_DESCRIPTION,
        ),
        Bound::new(
            "size",
            &params.size,
            "The size of the space, from distinct early echoes to a broad, slow bloom.",
        ),
        Bound::new(
            "diffusion",
            &params.diffusion,
            "From audible repeats to a dense wash.",
        ),
        Bound::new(
            "modrate",
            &params.mod_rate,
            "How fast the reverb tail drifts.",
        )
        // The Motion card is the modulation, so it paints what is left (B1); a host, the tooltip
        // and a screen reader read *Mod rate*.
        .labelled("Rate"),
        Bound::new(
            "modsync",
            &params.mod_sync,
            super::binding::SYNC_DESCRIPTION,
        ),
        Bound::new(
            "moddepth",
            &params.mod_depth,
            "How much the tail drifts; more thickens it.",
        )
        .labelled("Depth"),
        Bound::new("shift", &params.shift, "How far up the shimmer is pitched.")
            .law(mxm_preset::StepLaw::Semitones),
        Bound::new("shimmer", &params.shimmer, "How much shimmer."),
        Bound::new("placement", &params.placement, "Where the shimmer happens."),
        Bound::new(
            "reverse",
            &params.reverse,
            "Which way the shimmer's grains play.",
        ),
        Bound::new("regen", &params.regen, "How long the reverb lasts."),
        Bound::new(
            "lowcut",
            &params.low_cut,
            "Keeps the tail from getting muddy.",
        )
        .law(mxm_preset::StepLaw::Hertz),
        Bound::new(
            "highcut",
            &params.high_cut,
            "Darkens the tail as it goes on.",
        )
        .law(mxm_preset::StepLaw::Hertz),
        Bound::new(
            "freeze",
            &params.freeze,
            "Holds the current sound forever and lets no new sound in.",
        ),
        Bound::new(
            "mix",
            &params.mix,
            "The balance of dry sound and reverb; at zero the effect is off.",
        ),
    ]
}

/// What each option of a stepped control does, one sentence per cell in the parameter's own order
/// (design system §7.3; the owner, 2026-09-27: the cells of a row do not share one sentence).
fn details_of(id: &str) -> &'static [&'static str] {
    match id {
        "placement" => &[
            "Adds a steady harmony on top of the reverb.",
            "The shift repeats on every pass, climbing ever higher.",
            "The steady harmony and the climbing shimmer together.",
        ],
        "reverse" => &[
            "Each grain plays forwards.",
            "Each grain plays backwards before it is shifted.",
        ],
        _ => &[],
    }
}

fn bound<'a>(id: &str, params: &'a MxmShimmerParams) -> Bound<'a> {
    all_parameters(params)
        .into_iter()
        .find(|binding| binding.id == id)
        .expect("every drawn control is bound")
}

/// The cards' titles, in signal-flow order.
pub const TITLES: [&str; 4] = ["Space", "Motion", "Ascent", "Loop"];

/// What a leaf of this editor's cards draws. Hashed by what it names, which keeps its widget ids
/// stable when a card is re-paged.
#[derive(Clone, Copy, Debug, Hash)]
pub enum Leaf {
    Knob(&'static str),
    /// A stepped parameter's segmented switch.
    Switch(&'static str),
    /// An on/off parameter's toggle, its label the parameter's name.
    Toggle(&'static str),
    /// A control's tempo sync, the quarter note beside it.
    Picture(&'static str),
    Bloom,
}

/// A control's tempo sync, the quarter note on the grid of the knob beside it
/// (`plans/plan-tempo-sync-controls.md`).
fn sync_beside(id: &'static str) -> Node<Leaf> {
    mxm_ui::tree::switch_beside_knob(KNOB, tree::leaf(Leaf::Picture(id), Kind::SyncToggle))
}

/// The ladder of the control `id`, if it has a tempo sync.
fn ladder_of(id: &str) -> Option<mxm_tempo::Ladder> {
    match id {
        "predelay" => Some(crate::params::PRE_DELAY_SYNC),
        "modrate" => Some(crate::params::MOD_SYNC),
        _ => None,
    }
}

/// A segmented switch's cells: every option of the stepped parameter, as its own formatted text
/// (B4), so a host's list and the switch cannot disagree.
fn options(params: &MxmShimmerParams, id: &str) -> Vec<String> {
    let param = bound(id, params).param;
    let last = param
        .steps()
        .unwrap_or_else(|| unreachable!("`{id}` is not a switch"));
    (0..=last)
        .map(|option| param.format(option as f32 / last as f32))
        .collect()
}

/// The collection's knob row (`mxm_ui::tree::knob_row`): equal columns at the one knob column.
fn knob_row(ui: &Ui, params: &MxmShimmerParams, ids: &[&'static str]) -> Node<Leaf> {
    mxm_ui::tree::knob_row(
        ui,
        ids.iter()
            .map(|&id| {
                let bound = bound(id, params);
                let param = bound.param;
                // A syncable control's column holds its free readings and its divisions.
                let widest = match ladder_of(id) {
                    Some(ladder) => super::binding::synced_widest(param, ladder.span),
                    None => mxm_ui::control::widest_value(|n| param.format(n as f32)),
                };
                let knob = tree::leaf(
                    Leaf::Knob(id),
                    Kind::Knob {
                        name: bound.painted().to_owned(),
                        widest,
                        size: KNOB,
                        // In the collection's knob row, which sizes the columns.
                        column: 0.0,
                    },
                );
                (KNOB, knob)
            })
            .collect(),
    )
}

/// A segmented switch, its painted label the parameter's name.
fn switch(params: &MxmShimmerParams, id: &'static str) -> Node<Leaf> {
    tree::leaf(
        Leaf::Switch(id),
        Kind::Segmented {
            label: bound(id, params).param.name().to_owned(),
            options: options(params, id),
            beside: None,
        },
    )
}

/// An on/off parameter's toggle, labelled with its name: the collection draws every on/off this way
/// (design system §7.2), and its words are the parameter's.
fn toggle(params: &MxmShimmerParams, id: &'static str) -> Node<Leaf> {
    tree::leaf(
        Leaf::Toggle(id),
        Kind::Toggle {
            label: bound(id, params).painted().to_owned(),
        },
    )
}

/// Card `index`'s body, as a tree (plans/plan-layout-tree.md): described once, and that one
/// description is both measured — the card's floor and height — and drawn, leaf by leaf, through
/// the bindings ([`paint`]).
pub fn card(ui: &Ui, index: usize, params: &MxmShimmerParams) -> Node<Leaf> {
    match index {
        // The bloom, `SPACE_2` further from the knobs than the card's rhythm.
        0 => tree::stack(vec![
            tree::pad_all(
                0.0,
                0.0,
                SPACE_2,
                tree::leaf(
                    Leaf::Bloom,
                    Kind::Custom {
                        min_width: BLOOM_MIN_WIDTH,
                        height: Height::Fixed(TALL_PLOT_HEIGHT),
                        fills: true,
                    },
                ),
            ),
            // Pre-delay with its tempo sync beside it, one flat row with Size.
            tree::row_gap(
                ui.spacing().item_spacing.x,
                vec![
                    knob_row(ui, params, &["predelay"]),
                    sync_beside("predelaysync"),
                    knob_row(ui, params, &["size"]),
                ],
            ),
        ]),
        // Mod rate with its tempo sync beside it.
        1 => tree::stack(vec![tree::row_gap(
            ui.spacing().item_spacing.x,
            vec![
                knob_row(ui, params, &["diffusion", "modrate"]),
                sync_beside("modsync"),
                knob_row(ui, params, &["moddepth"]),
            ],
        )]),
        // Placement and Grain stacked, so the two share one cell width and read as a pair.
        2 => tree::stack(vec![
            knob_row(ui, params, &["shift", "shimmer"]),
            tree::share(
                Share::Cells,
                tree::stack(vec![switch(params, "placement"), switch(params, "reverse")]),
            ),
        ]),
        _ => tree::stack(vec![
            knob_row(ui, params, &["regen", "mix"]),
            knob_row(ui, params, &["lowcut", "highcut"]),
            toggle(params, "freeze"),
        ]),
    }
}

/// Everything a leaf draws with, and the shimmer level read once before the frame
/// (mxm-kit's `docs/plugin-conventions.md`, *Editor contract*: destructive telemetry is read once).
pub struct Live<'a, 'b> {
    pub params: &'a MxmShimmerParams,
    pub setter: &'a ParamSetter<'b>,
    pub text: &'a mut HashMap<&'static str, Option<String>>,
    pub level: f32,
    /// The host tempo in force: a synced knob reads its division with one.
    pub tempo: Option<f64>,
}

/// Draws one leaf, in the `Ui` the tree bounded to `rect`, through the bindings — so the controls,
/// their gestures and their names are exactly what they were.
pub fn paint(ui: &mut Ui, tokens: &Tokens, leaf: &Leaf, rect: Rect, live: &mut Live<'_, '_>) {
    let params = live.params;
    match *leaf {
        // Synced to a tempo, a knob reads its division; the host still reads its value.
        Leaf::Knob(id) => {
            let size = KNOB;
            let bound = bound(id, params);
            let division = {
                use nice_plug::prelude::Param as _;
                let synced: Option<(bool, &nice_plug::prelude::FloatParam, mxm_tempo::Ladder)> =
                    match id {
                        "predelay" => Some((
                            params.pre_delay_sync.value(),
                            &params.pre_delay,
                            crate::params::PRE_DELAY_SYNC,
                        )),
                        "modrate" => Some((
                            params.mod_sync.value(),
                            &params.mod_rate,
                            crate::params::MOD_SYNC,
                        )),
                        _ => None,
                    };
                synced
                    .filter(|(on, _, _)| *on)
                    .and_then(|(_, param, ladder)| {
                        ladder.shown(
                            param.unmodulated_normalized_value(),
                            live.tempo,
                            f64::from(param.preview_plain(0.0)),
                            f64::from(param.preview_plain(1.0)),
                        )
                    })
            };
            match division {
                Some(division) => bound.knob_with_reading(
                    ui,
                    tokens,
                    live.setter,
                    size,
                    rect.width(),
                    live.text,
                    division.label(),
                ),
                None => bound.knob(ui, tokens, live.setter, size, rect.width(), live.text),
            }
        }
        Leaf::Picture(id) => {
            super::binding::sync_picture(ui, tokens, id, bound(id, params).param, live.setter);
        }
        Leaf::Switch(id) => {
            let binding = bound(id, params);
            let labels = options(params, id);
            let options: Vec<&str> = labels.iter().map(String::as_str).collect();
            segmented(
                ui,
                tokens,
                id,
                binding.param,
                &options,
                None,
                details_of(id),
                live.setter,
            );
        }
        Leaf::Toggle(id) => {
            let binding = bound(id, params);
            toggle_labelled(
                ui,
                tokens,
                id,
                binding.param,
                binding.painted(),
                binding.description,
                live.setter,
                0.0,
            );
        }
        Leaf::Bloom => bloom(ui, tokens, params, live.level),
    }
}

pub fn cards(
    ui: &mut Ui,
    tokens: &Tokens,
    params: &MxmShimmerParams,
    setter: &ParamSetter<'_>,
    text: &mut HashMap<&'static str, Option<String>>,
    level: f32,
    tempo: Option<f64>,
) -> f32 {
    let items = page_items(ui, params);
    let text_editing = text.values().any(Option::is_some);
    let mut live = Live {
        params,
        setter,
        text,
        level,
        tempo,
    };
    let report = mxm_ui::paging::editor::show(
        ui,
        tokens,
        &items,
        &[],
        text_editing,
        &mut |ui, index| card(ui, index, params),
        &mut |ui, _, leaf, rect| paint(ui, tokens, leaf, rect, &mut live),
    );
    report
        .visible
        .iter()
        .map(|(_, rect)| rect.bottom())
        .fold(ui.min_rect().bottom(), f32::max)
}

/// The authored cards, each floor computed from its tree in `ui`'s fonts. Also what the keyboard
/// cursor is given: `mxm_ui::navigation::paged` reads the plan's own order from the last frame's
/// report and falls back to these before one exists.
pub fn page_items(ui: &Ui, params: &MxmShimmerParams) -> Vec<mxm_ui::paging::Item<'static>> {
    use mxm_ui::paging::{Category, Item, Key};
    TITLES
        .iter()
        .enumerate()
        .map(|(index, title)| Item {
            key: Key(index as u64),
            // As wide as its controls and no wider (`plans/plan-editor-standard.md` A1).
            card: {
                let floor = tree::card_floor(ui, title, &card(ui, index, params));
                mxm_ui::flow::Card::new(title, floor).capped(floor)
            },
            category: Category::Effects,
            kind: title,
        })
        .collect()
}

fn bloom(ui: &mut Ui, tokens: &Tokens, params: &MxmShimmerParams, level: f32) {
    let width = ui.available_width().max(BLOOM_MIN_WIDTH);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, TALL_PLOT_HEIGHT), egui::Sense::hover());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Other,
            true,
            "Reverb bloom and interval travel",
        )
    });
    response.on_hover_text(
        "The arcs show the reverb spreading; the rising marks show the shimmer climbing.",
    );
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CANVAS_RADIUS, tokens.surface_2);
    painter.rect_stroke(
        rect,
        CANVAS_RADIUS,
        egui::Stroke::new(AXIS_STROKE, tokens.border),
        egui::StrokeKind::Inside,
    );
    let centre = rect.center();
    let strength = (0.15 + level * 4.0).clamp(0.15, 1.0);
    let arc_stroke = TRACE_STROKE + strength * (EMPHASIS_STROKE - TRACE_STROKE);
    for pass in 0..5 {
        let radius = 12.0 + pass as f32 * (8.0 + params.size.value() * 3.0);
        let colour = if pass < 2 {
            tokens.accent
        } else {
            tokens.border_strong
        };
        painter.circle_stroke(centre, radius, egui::Stroke::new(arc_stroke, colour));
        let rise = params.shift.value() / 24.0;
        let x = centre.x - 34.0 + pass as f32 * 17.0;
        let y = centre.y + 24.0 - pass as f32 * rise * 9.0;
        painter.line_segment(
            [egui::pos2(x, centre.y + 26.0), egui::pos2(x, y)],
            egui::Stroke::new(EMPHASIS_STROKE, colour),
        );
    }
    if params.mix.value() == 0.0 {
        painter.text(
            rect.right_bottom() - egui::vec2(INNER_GUTTER, INNER_GUTTER),
            egui::Align2::RIGHT_BOTTOM,
            "Off",
            mxm_ui::typography::caption_style(ui.style()).resolve(ui.style()),
            tokens.text_secondary,
        );
    }
    if params.freeze.value() {
        painter.text(
            rect.left_bottom() + egui::vec2(INNER_GUTTER, -INNER_GUTTER),
            egui::Align2::LEFT_BOTTOM,
            "Held",
            mxm_ui::typography::caption_style(ui.style()).resolve(ui.style()),
            tokens.accent,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nice_plug::prelude::Params;

    #[test]
    fn every_parameter_is_bound_once() {
        let params = MxmShimmerParams::default();
        let ids: Vec<_> = all_parameters(&params)
            .iter()
            .map(|binding| binding.id)
            .collect();
        assert_eq!(ids.len(), params.param_map().len());
        for (id, _, _) in params.param_map() {
            assert_eq!(ids.iter().filter(|bound| **bound == id).count(), 1, "{id}");
        }
    }
}
