//! Live settings editor. Every change is written straight to the config file,
//! which the running overlay reloads within a few hundred milliseconds, so the
//! effect on screen updates while you are still holding the arrow key.

use anyhow::{Context, Result};
use pc_core::color::ColorMode;
use pc_core::{Config, Emitter, General, Rgba, Shape};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph, Tabs};
use ratatui::Frame;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use toml_edit::{value, Array, DocumentMut, Item, Table};

/// Writes are coalesced so holding an arrow key does not hammer the disk.
const WRITE_DEBOUNCE: Duration = Duration::from_millis(120);
const TABS: [&str; 3] = ["General", "Typing", "Deleting"];

struct GeneralField {
    label: &'static str,
    show: fn(&General) -> String,
    adjust: fn(&mut General, f32),
}

struct EmitterField {
    label: &'static str,
    show: fn(&Emitter, usize) -> String,
    adjust: fn(&mut Emitter, usize, f32),
}

fn step_f32(value: &mut f32, delta: f32, step: f32, min: f32, max: f32) {
    *value = (*value + delta * step).clamp(min, max);
}

fn step_u32(value: &mut u32, delta: f32, step: f32, min: u32, max: u32) {
    let next = (*value as f32 + delta * step).round();
    *value = (next.max(min as f32) as u32).min(max);
}

fn step_u64(value: &mut u64, delta: f32, step: f32, min: u64, max: u64) {
    let next = (*value as f64 + (delta * step) as f64).round();
    *value = (next.max(min as f64) as u64).min(max);
}

const GENERAL_FIELDS: &[GeneralField] = &[
    GeneralField {
        label: "enabled",
        show: |g| bool_label(g.enabled),
        adjust: |g, d| {
            if d != 0.0 {
                g.enabled = !g.enabled
            }
        },
    },
    GeneralField {
        label: "fps",
        show: |g| g.fps.to_string(),
        adjust: |g, d| step_u32(&mut g.fps, d, 5.0, 15, 240),
    },
    GeneralField {
        label: "max_particles",
        show: |g| g.max_particles.to_string(),
        adjust: |g, d| {
            let mut v = g.max_particles as u32;
            step_u32(&mut v, d, 50.0, 1, 20_000);
            g.max_particles = v as usize;
        },
    },
    GeneralField {
        label: "cursor_height_px",
        show: |g| format!("{:.1}", g.cursor_height_px),
        adjust: |g, d| step_f32(&mut g.cursor_height_px, d, 1.0, 1.0, 500.0),
    },
    GeneralField {
        label: "offset_x",
        show: |g| format!("{:.0}", g.offset_x),
        adjust: |g, d| step_f32(&mut g.offset_x, d, 1.0, -500.0, 500.0),
    },
    GeneralField {
        label: "offset_y",
        show: |g| format!("{:.0}", g.offset_y),
        adjust: |g, d| step_f32(&mut g.offset_y, d, 1.0, -500.0, 500.0),
    },
    GeneralField {
        label: "combo_enabled",
        show: |g| bool_label(g.combo_enabled),
        adjust: |g, d| {
            if d != 0.0 {
                g.combo_enabled = !g.combo_enabled
            }
        },
    },
    GeneralField {
        label: "combo_max_multiplier",
        show: |g| format!("{:.2}", g.combo_max_multiplier),
        adjust: |g, d| step_f32(&mut g.combo_max_multiplier, d, 0.1, 1.0, 20.0),
    },
    GeneralField {
        label: "min_emit_interval_ms",
        show: |g| g.min_emit_interval_ms.to_string(),
        adjust: |g, d| step_u64(&mut g.min_emit_interval_ms, d, 1.0, 0, 500),
    },
];

const SHAPES: [Shape; 7] = [
    Shape::Circle,
    Shape::Square,
    Shape::Triangle,
    Shape::Diamond,
    Shape::Star,
    Shape::Spark,
    Shape::Hexagon,
];

fn shape_name(shape: Shape) -> &'static str {
    match shape {
        Shape::Circle => "circle",
        Shape::Square => "square",
        Shape::Triangle => "triangle",
        Shape::Diamond => "diamond",
        Shape::Star => "star",
        Shape::Spark => "spark",
        Shape::Hexagon => "hexagon",
    }
}

fn bool_label(value: bool) -> String {
    if value { "on" } else { "off" }.to_string()
}

/// Nudges the HSV of the selected colour slot, or of the rainbow generator when
/// that mode is active.
fn adjust_hsv(color: &mut ColorMode, slot: usize, channel: usize, delta: f32) {
    if let Some((speed, saturation, value)) = color.rainbow_mut() {
        match channel {
            0 => *speed = (*speed + delta * 10.0).clamp(-2000.0, 2000.0),
            1 => *saturation = (*saturation + delta * 0.05).clamp(0.0, 1.0),
            _ => *value = (*value + delta * 0.05).clamp(0.0, 1.0),
        }
        return;
    }
    let Some(rgba) = color.slot(slot) else { return };
    let (mut h, mut s, mut v) = rgba.to_hsv();
    match channel {
        0 => h = (h + delta * 5.0).rem_euclid(360.0),
        1 => s = (s + delta * 0.05).clamp(0.0, 1.0),
        _ => v = (v + delta * 0.05).clamp(0.0, 1.0),
    }
    color.set_slot(slot, Rgba::from_hsv(h, s, v));
}

fn show_hsv(color: &ColorMode, slot: usize, channel: usize) -> String {
    match color {
        ColorMode::Rainbow {
            speed,
            saturation,
            value,
        } => match channel {
            0 => format!("{speed:.0} deg/s"),
            1 => format!("{saturation:.2}"),
            _ => format!("{value:.2}"),
        },
        _ => match color.slot(slot) {
            Some(rgba) => {
                let (h, s, v) = rgba.to_hsv();
                match channel {
                    0 => format!("{h:.0}"),
                    1 => format!("{s:.2}"),
                    _ => format!("{v:.2}"),
                }
            }
            None => "-".into(),
        },
    }
}

const EMITTER_FIELDS: &[EmitterField] = &[
    EmitterField {
        label: "enabled",
        show: |e, _| bool_label(e.enabled),
        adjust: |e, _, d| {
            if d != 0.0 {
                e.enabled = !e.enabled
            }
        },
    },
    EmitterField {
        label: "count",
        show: |e, _| e.count.to_string(),
        adjust: |e, _, d| step_u32(&mut e.count, d, 1.0, 0, 500),
    },
    EmitterField {
        label: "shape",
        show: |e, _| shape_name(e.shape).to_string(),
        adjust: |e, _, d| {
            if d == 0.0 {
                return;
            }
            let index = SHAPES.iter().position(|s| *s == e.shape).unwrap_or(0);
            let len = SHAPES.len();
            let next = if d > 0.0 {
                (index + 1) % len
            } else {
                (index + len - 1) % len
            };
            e.shape = SHAPES[next];
        },
    },
    EmitterField {
        label: "size_ratio",
        show: |e, _| format!("{:.2}", e.size_ratio),
        adjust: |e, _, d| step_f32(&mut e.size_ratio, d, 0.05, 0.01, 20.0),
    },
    EmitterField {
        label: "size_jitter",
        show: |e, _| format!("{:.2}", e.size_jitter),
        adjust: |e, _, d| step_f32(&mut e.size_jitter, d, 0.05, 0.0, 1.0),
    },
    EmitterField {
        label: "lifetime_ms",
        show: |e, _| e.lifetime_ms.to_string(),
        adjust: |e, _, d| step_u64(&mut e.lifetime_ms, d, 20.0, 16, 10_000),
    },
    EmitterField {
        label: "lifetime_jitter",
        show: |e, _| format!("{:.2}", e.lifetime_jitter),
        adjust: |e, _, d| step_f32(&mut e.lifetime_jitter, d, 0.05, 0.0, 1.0),
    },
    EmitterField {
        label: "speed",
        show: |e, _| format!("{:.0}", e.speed),
        adjust: |e, _, d| step_f32(&mut e.speed, d, 10.0, 0.0, 5000.0),
    },
    EmitterField {
        label: "speed_jitter",
        show: |e, _| format!("{:.2}", e.speed_jitter),
        adjust: |e, _, d| step_f32(&mut e.speed_jitter, d, 0.05, 0.0, 1.0),
    },
    EmitterField {
        label: "direction_deg",
        show: |e, _| format!("{:.0}", e.direction_deg),
        adjust: |e, _, d| step_f32(&mut e.direction_deg, d, 5.0, -360.0, 360.0),
    },
    EmitterField {
        label: "spread_deg",
        show: |e, _| format!("{:.0}", e.spread_deg),
        adjust: |e, _, d| step_f32(&mut e.spread_deg, d, 5.0, 0.0, 360.0),
    },
    EmitterField {
        label: "gravity",
        show: |e, _| format!("{:.0}", e.gravity),
        adjust: |e, _, d| step_f32(&mut e.gravity, d, 20.0, -3000.0, 3000.0),
    },
    EmitterField {
        label: "drag",
        show: |e, _| format!("{:.2}", e.drag),
        adjust: |e, _, d| step_f32(&mut e.drag, d, 0.1, 0.0, 20.0),
    },
    EmitterField {
        label: "rotation_speed",
        show: |e, _| format!("{:.1}", e.rotation_speed),
        adjust: |e, _, d| step_f32(&mut e.rotation_speed, d, 0.5, 0.0, 50.0),
    },
    EmitterField {
        label: "shrink",
        show: |e, _| bool_label(e.shrink),
        adjust: |e, _, d| {
            if d != 0.0 {
                e.shrink = !e.shrink
            }
        },
    },
    EmitterField {
        label: "color mode",
        show: |e, _| e.color.mode_name().to_string(),
        adjust: |e, _, d| {
            if d != 0.0 {
                e.color.cycle(d > 0.0)
            }
        },
    },
    EmitterField {
        label: "colour slot",
        show: |e, slot| {
            let count = e.color.slot_count();
            if count == 0 {
                "n/a".into()
            } else {
                format!("{} / {}   {}", slot + 1, count, hex_of(e, slot))
            }
        },
        adjust: |_, _, _| {},
    },
    EmitterField {
        label: "hue / rainbow speed",
        show: |e, slot| show_hsv(&e.color, slot, 0),
        adjust: |e, slot, d| adjust_hsv(&mut e.color, slot, 0, d),
    },
    EmitterField {
        label: "saturation",
        show: |e, slot| show_hsv(&e.color, slot, 1),
        adjust: |e, slot, d| adjust_hsv(&mut e.color, slot, 1, d),
    },
    EmitterField {
        label: "value",
        show: |e, slot| show_hsv(&e.color, slot, 2),
        adjust: |e, slot, d| adjust_hsv(&mut e.color, slot, 2, d),
    },
];

fn hex_of(emitter: &Emitter, slot: usize) -> String {
    emitter
        .color
        .slot(slot)
        .map(|c| c.to_hex())
        .unwrap_or_else(|| "-".into())
}

struct App {
    config: Config,
    path: PathBuf,
    tab: usize,
    selected: [usize; 3],
    slot: usize,
    dirty: bool,
    last_write: Instant,
    status: String,
    quit: bool,
}

impl App {
    fn rows(&self) -> usize {
        if self.tab == 0 {
            GENERAL_FIELDS.len()
        } else {
            EMITTER_FIELDS.len()
        }
    }

    fn emitter(&self) -> &Emitter {
        if self.tab == 1 {
            &self.config.typing
        } else {
            &self.config.deleting
        }
    }

    fn emitter_mut(&mut self) -> &mut Emitter {
        if self.tab == 1 {
            &mut self.config.typing
        } else {
            &mut self.config.deleting
        }
    }

    fn adjust(&mut self, delta: f32) {
        let index = self.selected[self.tab];
        if self.tab == 0 {
            (GENERAL_FIELDS[index].adjust)(&mut self.config.general, delta);
        } else {
            let slot = self.slot;
            (EMITTER_FIELDS[index].adjust)(self.emitter_mut(), slot, delta);
            let count = self.emitter().color.slot_count();
            self.slot = self.slot.min(count.saturating_sub(1));
        }
        self.dirty = true;
    }

    /// Rewrites values in place with toml_edit so the comments that document
    /// every setting survive an editing session.
    fn save(&mut self) -> Result<()> {
        let source = std::fs::read_to_string(&self.path)
            .unwrap_or_else(|_| pc_core::config::DEFAULT_TOML.to_string());
        let mut doc: DocumentMut = source
            .parse()
            .context("the existing config is not valid TOML; fix or delete it first")?;

        apply(&mut doc, &self.config);

        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.path, doc.to_string())
            .with_context(|| format!("writing {}", self.path.display()))?;
        self.dirty = false;
        self.last_write = Instant::now();
        Ok(())
    }
}

fn apply(doc: &mut DocumentMut, config: &Config) {
    let g = &config.general;
    doc["general"]["enabled"] = value(g.enabled);
    doc["general"]["fps"] = value(g.fps as i64);
    doc["general"]["max_particles"] = value(g.max_particles as i64);
    doc["general"]["cursor_height_px"] = value(g.cursor_height_px as f64);
    doc["general"]["offset_x"] = value(g.offset_x as f64);
    doc["general"]["offset_y"] = value(g.offset_y as f64);
    doc["general"]["combo_enabled"] = value(g.combo_enabled);
    doc["general"]["combo_window_ms"] = value(g.combo_window_ms as i64);
    doc["general"]["combo_max_multiplier"] = value(g.combo_max_multiplier as f64);
    doc["general"]["min_emit_interval_ms"] = value(g.min_emit_interval_ms as i64);

    apply_emitter(doc, "typing", &config.typing);
    apply_emitter(doc, "deleting", &config.deleting);
}

fn apply_emitter(doc: &mut DocumentMut, section: &str, e: &Emitter) {
    doc[section]["enabled"] = value(e.enabled);
    doc[section]["count"] = value(e.count as i64);
    doc[section]["shape"] = value(shape_name(e.shape));
    doc[section]["size_ratio"] = value(e.size_ratio as f64);
    doc[section]["size_jitter"] = value(e.size_jitter as f64);
    doc[section]["lifetime_ms"] = value(e.lifetime_ms as i64);
    doc[section]["lifetime_jitter"] = value(e.lifetime_jitter as f64);
    doc[section]["speed"] = value(e.speed as f64);
    doc[section]["speed_jitter"] = value(e.speed_jitter as f64);
    doc[section]["direction_deg"] = value(e.direction_deg as f64);
    doc[section]["spread_deg"] = value(e.spread_deg as f64);
    doc[section]["gravity"] = value(e.gravity as f64);
    doc[section]["drag"] = value(e.drag as f64);
    doc[section]["rotation_speed"] = value(e.rotation_speed as f64);
    doc[section]["shrink"] = value(e.shrink);

    // Each mode has its own keys, so the colour table is replaced wholesale
    // rather than patched key by key.
    doc[section]["color"] = Item::Table(color_table(&e.color));
}

fn color_table(mode: &ColorMode) -> Table {
    let mut table = Table::new();
    match mode {
        ColorMode::Fixed { color } => {
            table["mode"] = value("fixed");
            table["color"] = value(color.as_str());
        }
        ColorMode::Palette { colors } => {
            table["mode"] = value("palette");
            let mut array = Array::new();
            for color in colors {
                array.push(color.as_str());
            }
            table["colors"] = value(array);
        }
        ColorMode::Gradient { from, to } => {
            table["mode"] = value("gradient");
            table["from"] = value(from.as_str());
            table["to"] = value(to.as_str());
        }
        ColorMode::Rainbow {
            speed,
            saturation,
            value: v,
        } => {
            table["mode"] = value("rainbow");
            table["speed"] = value(*speed as f64);
            table["saturation"] = value(*saturation as f64);
            table["value"] = value(*v as f64);
        }
    }
    table
}

pub fn run(path: PathBuf) -> Result<()> {
    let config = if path.exists() {
        Config::load_from(&path)?
    } else {
        Config::default()
    };

    let mut app = App {
        config,
        path,
        tab: 1,
        selected: [0; 3],
        slot: 0,
        dirty: false,
        last_write: Instant::now(),
        status: "arrows adjust, saved automatically".into(),
        quit: false,
    };

    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, &mut app);
    ratatui::restore();

    // A pending edit must not be lost just because the debounce had not elapsed.
    if app.dirty {
        app.save()?;
    }
    result
}

fn event_loop(terminal: &mut ratatui::DefaultTerminal, app: &mut App) -> Result<()> {
    loop {
        terminal.draw(|frame| draw(frame, app))?;

        if event::poll(Duration::from_millis(60))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    handle_key(app, key.code, key.modifiers);
                }
            }
        }

        if app.dirty && app.last_write.elapsed() >= WRITE_DEBOUNCE {
            match app.save() {
                Ok(()) => app.status = format!("saved {}", app.path.display()),
                Err(err) => app.status = format!("save failed: {err}"),
            }
        }
        if app.quit {
            return Ok(());
        }
    }
}

fn handle_key(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    // Shift makes every adjustment ten times coarser.
    let scale = if modifiers.contains(KeyModifiers::SHIFT) {
        10.0
    } else {
        1.0
    };
    match code {
        KeyCode::Char('q') | KeyCode::Esc => app.quit = true,
        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => app.quit = true,
        KeyCode::Tab => {
            app.tab = (app.tab + 1) % TABS.len();
            app.slot = 0;
        }
        KeyCode::BackTab => {
            app.tab = (app.tab + TABS.len() - 1) % TABS.len();
            app.slot = 0;
        }
        KeyCode::Up => {
            let rows = app.rows();
            let selected = &mut app.selected[app.tab];
            *selected = (*selected + rows - 1) % rows;
        }
        KeyCode::Down => {
            let rows = app.rows();
            let selected = &mut app.selected[app.tab];
            *selected = (*selected + 1) % rows;
        }
        KeyCode::Left | KeyCode::Char('-') => app.adjust(-scale),
        KeyCode::Right | KeyCode::Char('+') | KeyCode::Char('=') => app.adjust(scale),
        KeyCode::Enter | KeyCode::Char(' ') => app.adjust(scale),
        KeyCode::Char('[') => {
            if app.tab != 0 {
                let count = app.emitter().color.slot_count();
                if count > 0 {
                    app.slot = (app.slot + count - 1) % count;
                }
            }
        }
        KeyCode::Char(']') => {
            if app.tab != 0 {
                let count = app.emitter().color.slot_count();
                if count > 0 {
                    app.slot = (app.slot + 1) % count;
                }
            }
        }
        KeyCode::Char('a') => {
            if app.tab != 0 {
                app.emitter_mut().color.add_slot();
                app.dirty = true;
            }
        }
        KeyCode::Char('x') => {
            if app.tab != 0 {
                let slot = app.slot;
                app.emitter_mut().color.remove_slot(slot);
                let count = app.emitter().color.slot_count();
                app.slot = app.slot.min(count.saturating_sub(1));
                app.dirty = true;
            }
        }
        KeyCode::Char('r') => {
            app.config = Config::default();
            app.slot = 0;
            app.dirty = true;
            app.status = "reset to defaults".into();
        }
        KeyCode::Char('s') => {
            app.dirty = true;
            app.last_write = Instant::now() - WRITE_DEBOUNCE;
        }
        _ => {}
    }
}

fn draw(frame: &mut Frame, app: &App) {
    let areas = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(6),
        Constraint::Length(6),
        Constraint::Length(3),
    ])
    .split(frame.area());

    let tabs = Tabs::new(TABS.iter().map(|t| Span::raw(*t)).collect::<Vec<_>>())
        .block(Block::bordered().title(" imlec tune "))
        .select(app.tab)
        .highlight_style(Style::default().fg(Color::Black).bg(Color::Cyan));
    frame.render_widget(tabs, areas[0]);

    render_fields(frame, app, areas[1]);
    render_preview(frame, app, areas[2]);

    let help = Line::from(vec![
        Span::styled("←/→", Style::default().fg(Color::Cyan)),
        Span::raw(" adjust  "),
        Span::styled("shift", Style::default().fg(Color::Cyan)),
        Span::raw(" x10  "),
        Span::styled("↑/↓", Style::default().fg(Color::Cyan)),
        Span::raw(" field  "),
        Span::styled("tab", Style::default().fg(Color::Cyan)),
        Span::raw(" section  "),
        Span::styled("[ ]", Style::default().fg(Color::Cyan)),
        Span::raw(" colour slot  "),
        Span::styled("a/x", Style::default().fg(Color::Cyan)),
        Span::raw(" add/remove  "),
        Span::styled("r", Style::default().fg(Color::Cyan)),
        Span::raw(" reset  "),
        Span::styled("q", Style::default().fg(Color::Cyan)),
        Span::raw(" quit"),
    ]);
    frame.render_widget(
        Paragraph::new(help).block(Block::bordered().title(format!(" {} ", app.status))),
        areas[3],
    );
}

fn render_fields(frame: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = if app.tab == 0 {
        GENERAL_FIELDS
            .iter()
            .map(|f| row(f.label, (f.show)(&app.config.general)))
            .collect()
    } else {
        let emitter = app.emitter();
        EMITTER_FIELDS
            .iter()
            .map(|f| row(f.label, (f.show)(emitter, app.slot)))
            .collect()
    };

    let mut state = ListState::default();
    state.select(Some(app.selected[app.tab]));
    let list = List::new(items)
        .block(Block::bordered().title(format!(" {} ", TABS[app.tab])))
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("› ");
    frame.render_stateful_widget(list, area, &mut state);
}

fn row(label: &str, value: String) -> ListItem<'static> {
    ListItem::new(Line::from(vec![
        Span::raw(format!("{label:<22}")),
        Span::styled(value, Style::default().fg(Color::Yellow)),
    ]))
}

fn render_preview(frame: &mut Frame, app: &App, area: Rect) {
    let mut lines = Vec::new();

    if app.tab == 0 {
        let g = &app.config.general;
        lines.push(Line::from(format!(
            "particle size = {:.1} px   ({:.1} x {:.2})",
            g.cursor_height_px * app.config.typing.size_ratio,
            g.cursor_height_px,
            app.config.typing.size_ratio
        )));
        lines.push(Line::from(format!(
            "budget {} particles at {} fps",
            g.max_particles, g.fps
        )));
    } else {
        let emitter = app.emitter();
        let size = app.config.general.cursor_height_px * emitter.size_ratio;
        lines.push(Line::from(format!(
            "{} · {:.1} px · {} ms · {:.0} px/s",
            shape_name(emitter.shape),
            size,
            emitter.lifetime_ms,
            emitter.speed
        )));

        let mut swatches = vec![Span::raw("colours  ")];
        match emitter.color.slot_count() {
            0 => swatches.push(Span::raw("rainbow (generated)")),
            count => {
                for index in 0..count {
                    let Some(rgba) = emitter.color.slot(index) else {
                        continue;
                    };
                    let [r, g, b, _] = rgba.to_rgba8();
                    let marker = if index == app.slot {
                        "▐██▌"
                    } else {
                        " ██ "
                    };
                    swatches.push(Span::styled(
                        marker,
                        Style::default().fg(Color::Rgb(r, g, b)),
                    ));
                }
            }
        }
        lines.push(Line::from(swatches));
        lines.push(Line::from(
            format!(
                "cone {:.0}° ± {:.0}°   gravity {:.0}",
                emitter.direction_deg,
                emitter.spread_deg / 2.0,
                emitter.gravity
            )
            .dim(),
        ));
    }

    lines.push(Line::from(
        "changes reach the running overlay within half a second".dim(),
    ));
    frame.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(" preview ")),
        area,
    );
}
