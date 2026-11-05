//! The root view: the Harp main interface (grouped single-row sidebar + rounded terminal
//! card), drawn with GPUI primitives at the sizes and token values of `theme.rs`.

use gpui::prelude::FluentBuilder;
use gpui::{
    div, point, px, relative, BoxShadow, Context, Div, FontWeight, Hsla, InteractiveElement,
    IntoElement, ParentElement, Render, Rgba, StatefulInteractiveElement, Styled, Window,
};
use pigeon_core::api::{Message, SessionInfo, StatusState as State};
use std::collections::BTreeMap;

use crate::host::HostLink;
use crate::icons::{self, TopSymbol};
use crate::sidebar::{self, Row};
use crate::status::{StatusModel, Summary};
use crate::terminal::{self, Palette as TermPalette, Screen};
use crate::theme::{fonts, metrics, Palette};

pub struct Workspace {
    host: HostLink,
    dark: bool,
    sessions: Vec<SessionInfo>,
    selected: Option<u32>,
    status: StatusModel,
    screen: Option<Screen>,
    attached_pane: Option<u32>,
    request: u32,
    /// Set when the connection drops; replaces the status in the card header.
    disconnected: Option<String>,
}

fn shadow(color: Rgba, y: f32, blur: f32) -> BoxShadow {
    BoxShadow {
        color: Hsla::from(color),
        offset: point(px(0.), px(y)),
        blur_radius: px(blur),
        spread_radius: px(0.),
        inset: false,
    }
}

/// `Working · 1m`, the macOS `SidebarModel.subtitle` wording.
fn status_line(summary: &Summary) -> Option<String> {
    let label = match summary.state? {
        State::Blocked => "Needs you",
        State::Error => "Error",
        State::Done => "Done",
        State::Working => "Working",
        State::Idle => "Idle",
    };
    Some(format!(
        "{label} · {}",
        sidebar::age(summary.age_ms.unwrap_or(0))
    ))
}

/// The cwd as the header shows it: `$HOME` folded to `~`; the element truncates from the start.
fn display_path(cwd: &str) -> String {
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && cwd.starts_with(&home) => {
            format!("~{}", &cwd[home.len()..])
        }
        _ => cwd.to_owned(),
    }
}

impl Workspace {
    pub fn new(host: HostLink, dark: bool, hello: &Message) -> Self {
        let mut workspace = Self {
            host,
            dark,
            sessions: Vec::new(),
            selected: None,
            status: StatusModel::default(),
            screen: None,
            attached_pane: None,
            request: 0,
            disconnected: None,
        };
        workspace.ingest(hello);
        workspace
    }

    pub fn set_dark(&mut self, dark: bool) {
        self.dark = dark;
    }

    fn pane_of(&self, session_id: u32) -> Option<u32> {
        self.sessions
            .iter()
            .find(|s| s.session_id == session_id)
            .and_then(|s| s.active_pane.or_else(|| s.panes.first().copied()))
    }

    fn rows(&self) -> Vec<Row> {
        sidebar::rows(&self.sessions, &BTreeMap::new(), &|panes| {
            self.status.summary(panes)
        })
    }

    fn select(&mut self, session_id: u32) {
        self.selected = Some(session_id);
        self.attach_selected();
    }

    fn attach_selected(&mut self) {
        let Some(pane) = self.selected.and_then(|id| self.pane_of(id)) else {
            return;
        };
        if self.attached_pane == Some(pane) {
            return;
        }
        self.request = self.request.saturating_add(1);
        self.host.send(Message::Attach {
            id: self.request,
            pane_id: pane,
            size_policy: None,
            history: Some(false),
            since: None,
        });
        self.attached_pane = Some(pane);
        self.screen = None;
    }

    /// Folds one host message into the view state.
    pub fn ingest(&mut self, message: &Message) {
        match message {
            Message::Sessions { items } => {
                self.sessions = items.clone();
                let still_there = self
                    .selected
                    .is_some_and(|id| self.sessions.iter().any(|s| s.session_id == id));
                if !still_there {
                    // The first session row in sidebar order, not the host's list order.
                    self.selected = self.rows().iter().find_map(|row| match row {
                        Row::Session { session_id, .. } => Some(*session_id),
                        Row::Header { .. } => None,
                    });
                }
                self.attach_selected();
            }
            Message::StatusSnapshot { .. } => {
                self.status.apply(message);
            }
            Message::Snapshot { .. } => {
                if let Some(screen) = &mut self.screen {
                    screen.apply_snapshot(message);
                } else if let Some(screen) = Screen::from_message(message) {
                    if Some(screen.pane_id) == self.attached_pane {
                        self.screen = Some(screen);
                    }
                }
            }
            Message::Delta { .. } => {
                if let Some(screen) = &mut self.screen {
                    if !screen.apply_delta(message) {
                        self.attached_pane = None;
                        self.attach_selected();
                    }
                }
            }
            _ => {}
        }
    }

    pub fn disconnected(&mut self, reason: String) {
        self.disconnected = Some(reason);
    }

    pub fn request_sessions(&self) {
        self.host.send(Message::Sessions { items: Vec::new() });
    }

    fn top_bar(&self, palette: &Palette) -> Div {
        let button = |id: &'static str, symbol: TopSymbol| {
            div()
                .id(id)
                .size(px(metrics::TOP_BUTTON_SIZE))
                .rounded(px(metrics::TOP_BUTTON_RADIUS))
                .flex()
                .items_center()
                .justify_center()
                .hover(|style| style.bg(palette.hover))
                .child(icons::top_symbol(symbol, palette.gray))
        };
        div()
            .h(px(metrics::HEADER_HEIGHT))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_end()
            .pr(px(metrics::TOP_BAR_PAD_RIGHT))
            .gap(px(metrics::TOP_BUTTON_GAP))
            .child(button("toggle-sidebar", TopSymbol::Sidebar))
            .child(button("new-session", TopSymbol::Plus))
    }

    fn header_row(
        &self,
        palette: &Palette,
        name: String,
        count: usize,
        needs: usize,
        first: bool,
    ) -> Div {
        div()
            .h(px(metrics::SECTION_HEADER_HEIGHT))
            .when(!first, |d| d.mt(px(metrics::SECTION_GAP)))
            .pl(px(metrics::ROW_PAD_LEFT))
            .pr(px(metrics::ROW_PAD_RIGHT))
            .flex()
            .items_center()
            .text_size(px(metrics::SECTION_TITLE_SIZE))
            .child(
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(palette.section_name)
                    .child(name),
            )
            .child(
                div()
                    .ml(px(6.))
                    .text_color(palette.section_count)
                    .child(count.to_string()),
            )
            .child(div().flex_1())
            .when(needs > 0, |d| {
                d.child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette.needs_text)
                        .child(sidebar::needs_label(needs)),
                )
            })
    }

    #[allow(clippy::too_many_arguments)]
    fn session_row(
        &self,
        palette: &Palette,
        session_id: u32,
        name: String,
        glyph: Option<icons::StatusGlyph>,
        unread: bool,
        age: Option<String>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected = self.selected == Some(session_id);
        div()
            .id(("session", session_id as usize))
            .h(px(metrics::ROW_HEIGHT))
            .mb(px(metrics::ROW_GAP))
            .pl(px(metrics::ROW_PAD_LEFT))
            .pr(px(metrics::ROW_PAD_RIGHT))
            .flex()
            .items_center()
            .gap(px(metrics::ROW_GAP_INNER))
            .rounded(px(metrics::ROW_RADIUS))
            .when(selected, |row| {
                row.bg(palette.selected)
                    .border(px(0.5))
                    .border_color(palette.selected_edge)
                    .shadow(vec![shadow(palette.selected_shadow, 1., 2.)])
            })
            .when(!selected, |row| row.hover(|style| style.bg(palette.hover)))
            .on_click(cx.listener(move |workspace, _, _, cx| {
                workspace.select(session_id);
                cx.notify();
            }))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(metrics::ROW_TITLE_SIZE))
                    .text_color(palette.ink)
                    .when(unread, |n| n.font_weight(FontWeight::SEMIBOLD))
                    .child(name),
            )
            .when_some(age, |row, age| {
                row.child(
                    div()
                        .flex_shrink_0()
                        .text_size(px(metrics::TIME_SIZE))
                        .font_features(fonts::tabular())
                        .text_color(palette.gray)
                        .child(age),
                )
            })
            .child(icons::status(glyph, palette, self.dark))
    }

    fn render_sidebar(&self, palette: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let mut list = div()
            .px(px(metrics::LIST_SIDE_PADDING))
            .pt(px(metrics::LIST_TOP_PADDING))
            .flex()
            .flex_col();
        for row in self.rows() {
            list = match row {
                Row::Header {
                    name,
                    count,
                    needs,
                    first,
                } => list.child(self.header_row(palette, name, count, needs, first)),
                Row::Session {
                    session_id,
                    name,
                    glyph,
                    unread,
                    age,
                    ..
                } => {
                    list.child(self.session_row(palette, session_id, name, glyph, unread, age, cx))
                }
            };
        }
        div()
            .w(px(metrics::SIDEBAR_WIDTH))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(self.top_bar(palette))
            .child(list)
    }

    fn render_card(&self, palette: &Palette) -> impl IntoElement {
        let session = self
            .selected
            .and_then(|id| self.sessions.iter().find(|s| s.session_id == id));
        let title = session
            .map(sidebar::session_label)
            .unwrap_or_else(|| "No session".to_owned());
        let path = session.map(|s| display_path(&s.cwd)).unwrap_or_default();
        let right = match &self.disconnected {
            Some(reason) => Some(reason.clone()),
            None => session.and_then(|s| status_line(&self.status.summary(&s.panes))),
        };

        let header = div()
            .h(px(metrics::CARD_HEADER_HEIGHT))
            .flex_shrink_0()
            .pl(px(metrics::CARD_HEADER_PAD_OPEN))
            .pr(px(metrics::HEADER_PAD_RIGHT))
            .flex()
            .items_center()
            .gap(px(8.))
            .border_b(px(metrics::CARD_STROKE))
            .border_color(palette.divider)
            .text_size(px(metrics::ROW_TITLE_SIZE))
            .child(
                div()
                    .flex_shrink_0()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette.ink)
                    .child(title),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .text_ellipsis_start()
                    .text_color(palette.gray)
                    .child(path),
            )
            .when_some(right, |header, right| {
                header.child(
                    div()
                        .flex_shrink_0()
                        .max_w(relative(0.45))
                        .truncate()
                        .text_size(px(metrics::CARD_STATUS_SIZE))
                        .text_color(palette.gray)
                        .child(right),
                )
            });

        let body = div()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .pl(px(metrics::TERMINAL_PAD_LEFT))
            .pt(px(8.))
            .font_family(fonts::mono())
            .when_some(self.screen.as_ref(), |body, screen| {
                body.child(terminal::grid(screen, &TermPalette::resolve(self.dark)))
            });

        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .pt(px(metrics::CARD_INSET))
            .pr(px(metrics::CARD_INSET))
            .pb(px(metrics::CARD_INSET))
            .child(
                div()
                    .size_full()
                    .rounded(px(metrics::CARD_RADIUS))
                    .border(px(metrics::CARD_STROKE))
                    .border_color(palette.card_edge)
                    .shadow(vec![shadow(palette.card_shadow, 4., 16.)])
                    .bg(palette.terminal_bg)
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .child(header)
                    .child(body),
            )
    }
}

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::resolve(self.dark);
        div()
            .flex()
            .size_full()
            .font_family(fonts::ui())
            .bg(palette.sidebar_surface)
            .child(self.render_sidebar(&palette, cx))
            .child(self.render_card(&palette))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_line_uses_the_macos_wording() {
        let working = Summary {
            state: Some(State::Working),
            unread: 0,
            age_ms: Some(61_000),
        };
        assert_eq!(status_line(&working).as_deref(), Some("Working · 1m"));
        let blocked = Summary {
            state: Some(State::Blocked),
            unread: 0,
            age_ms: Some(0),
        };
        assert_eq!(status_line(&blocked).as_deref(), Some("Needs you · now"));
        assert_eq!(status_line(&Summary::default()), None);
    }
}
