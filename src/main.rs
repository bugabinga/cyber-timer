use chrono::{DateTime, Duration as ChronoDuration, Local};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    buffer::Buffer,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Style, Stylize},
    widgets::{Block, BorderType, Borders, Paragraph, Row, Table, Widget},
    Terminal,
};
use regex::Regex;
use std::{
    env, io,
    process::Command,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

const FALLBACK_SOUND: &str = "/usr/share/sounds/freedesktop/stereo/complete.oga";

mod colors {
    pub const BG: ratatui::style::Color = ratatui::style::Color::Rgb(8, 8, 15);
    pub const BG_CARD: ratatui::style::Color = ratatui::style::Color::Rgb(15, 15, 28);
    pub const BORDER: ratatui::style::Color = ratatui::style::Color::Rgb(255, 0, 128);
    pub const PINK: ratatui::style::Color = ratatui::style::Color::Rgb(255, 0, 128);
    pub const PINK_HOT: ratatui::style::Color = ratatui::style::Color::Rgb(255, 20, 147);
    pub const PINK_NEON: ratatui::style::Color = ratatui::style::Color::Rgb(255, 0, 255);
    pub const CYAN: ratatui::style::Color = ratatui::style::Color::Rgb(0, 255, 255);
    pub const CYAN_ELEC: ratatui::style::Color = ratatui::style::Color::Rgb(0, 212, 255);
    pub const PURPLE: ratatui::style::Color = ratatui::style::Color::Rgb(147, 0, 211);
    pub const PURPLE_NEON: ratatui::style::Color = ratatui::style::Color::Rgb(191, 0, 255);
    pub const GREEN: ratatui::style::Color = ratatui::style::Color::Rgb(0, 255, 127);
    pub const YELLOW: ratatui::style::Color = ratatui::style::Color::Rgb(255, 215, 0);
    pub const ORANGE: ratatui::style::Color = ratatui::style::Color::Rgb(255, 140, 0);
    pub const RED: ratatui::style::Color = ratatui::style::Color::Rgb(255, 50, 50);
    pub const TEXT: ratatui::style::Color = ratatui::style::Color::Rgb(200, 200, 220);
    pub const TEXT_DIM: ratatui::style::Color = ratatui::style::Color::Rgb(100, 100, 130);
    pub const WHITE: ratatui::style::Color = ratatui::style::Color::Rgb(255, 255, 255);
}

#[derive(Clone)]
struct TimerState {
    label: String,
    total: Duration,
    end_time: DateTime<Local>,
    alert_triggered: bool,
}

fn parse_duration(s: &str) -> Duration {
    let re = Regex::new(r"(\d+)(sec|s|min|h)").unwrap();
    if let Some(caps) = re.captures(s) {
        let val: u64 = caps[1].parse().unwrap_or(10);
        match &caps[2] {
            "min" => Duration::from_secs(val * 60),
            "h" => Duration::from_secs(val * 3600),
            _ => Duration::from_secs(val),
        }
    } else {
        Duration::from_secs(10)
    }
}

fn parse_labeled_duration(s: &str) -> (String, Duration) {
    if let Some((label, duration_str)) = s.split_once(':') {
        (label.to_string(), parse_duration(duration_str))
    } else {
        ("Timer".to_string(), parse_duration(s))
    }
}

fn do_alert(label: &str) {
    let _ = Command::new("notify-send")
        .args([
            "-t",
            "6000",
            "-i",
            "alarm-clock",
            "🎯 Timer Complete!",
            label,
        ])
        .spawn();
    let _ = Command::new("pw-play").arg(FALLBACK_SOUND).spawn();
}

fn create_animated_bar(progress: f64, width: usize, elapsed_ms: u64) -> (String, Color) {
    let filled = ((progress * width as f64) as usize).min(width);
    let empty = width.saturating_sub(filled);

    let phase = (elapsed_ms / 200) % 8;

    let (main_color, _gradient) = if progress >= 1.0 {
        (
            colors::PINK_NEON,
            vec![colors::PINK_NEON, colors::PURPLE_NEON, colors::CYAN],
        )
    } else if progress > 0.75 {
        (colors::GREEN, vec![colors::CYAN_ELEC, colors::GREEN])
    } else if progress > 0.5 {
        (colors::YELLOW, vec![colors::YELLOW, colors::GREEN])
    } else if progress > 0.25 {
        (colors::ORANGE, vec![colors::ORANGE, colors::YELLOW])
    } else {
        (colors::RED, vec![colors::RED, colors::ORANGE])
    };

    let mut bar = String::new();

    // Animated leading edge
    if filled > 0 && filled < width {
        let edge_char = match phase {
            0 => "▓",
            1 => "▒",
            2 => "░",
            3 => "▒",
            4 => "▓",
            5 => "█",
            6 => "▓",
            _ => "▒",
        };
        bar.push_str(&format!("{}", edge_char).repeat(filled - 1));
        bar.push('█');
    } else {
        bar.push_str(&"█".repeat(filled));
    }

    bar.push_str(&"░".repeat(empty));

    (bar, main_color)
}

fn format_time_remaining(secs: i64) -> String {
    if secs >= 3600 {
        format!(
            "{:02}:{:02}:{:02}",
            secs / 3600,
            (secs % 3600) / 60,
            secs % 60
        )
    } else if secs >= 60 {
        format!("{:02}:{:02}", secs / 60, secs % 60)
    } else {
        format!("00:{:02}", secs)
    }
}

struct TimerWidget<'a> {
    states: &'a [TimerState],
    now: DateTime<Local>,
    elapsed_ms: u64,
}

impl<'a> TimerWidget<'a> {
    fn new(states: &'a [TimerState], now: DateTime<Local>, elapsed_ms: u64) -> Self {
        Self {
            states,
            now,
            elapsed_ms,
        }
    }
}

impl<'a> Widget for TimerWidget<'a> {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut Buffer) {
        let width = area.width as usize;

        // Layout: header | main progress | footer
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(4), // Header
                Constraint::Min(10),   // Progress display
                Constraint::Length(5), // Timer list
                Constraint::Length(3), // Footer
            ])
            .split(area);

        // ═══════════════════════════════════════════════════════════════
        // HEADER
        // ═══════════════════════════════════════════════════════════════
        let border_style = Style::default().fg(colors::BORDER);

        let header_block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .border_type(BorderType::Thick);
        header_block.render(chunks[0], buf);

        let time_str = self.now.format("%H:%M:%S").to_string();

        // Title row
        let title = format!(
            "  ╔══════════╦══════════════════╦══════════╗  {}  CyberTimer v0.1.0  {}  ",
            "⚡", "⌁"
        );
        let title_area = ratatui::prelude::Rect {
            x: chunks[0].x + 1,
            y: chunks[0].y,
            width: chunks[0].width - 2,
            height: 1,
        };
        Paragraph::new(title)
            .style(Style::default().fg(colors::TEXT_DIM).bg(colors::BG_CARD))
            .alignment(Alignment::Center)
            .render(title_area, buf);

        // Clock row
        let clock = format!("  ║    {}    ║   ACTIVE TIMERS  ║  {}  ║", time_str, "♥");
        let clock_area = ratatui::prelude::Rect {
            x: chunks[0].x + 1,
            y: chunks[0].y + 1,
            width: chunks[0].width - 2,
            height: 1,
        };
        Paragraph::new(clock)
            .style(Style::default().fg(colors::CYAN).bold().bg(colors::BG_CARD))
            .alignment(Alignment::Center)
            .render(clock_area, buf);

        // Subtitle
        let subtitle = format!("  ╚══════════╩══════════════════╩══════════╝  ⏱  RUST POWERED  ⏱");
        let subtitle_area = ratatui::prelude::Rect {
            x: chunks[0].x + 1,
            y: chunks[0].y + 2,
            width: chunks[0].width - 2,
            height: 1,
        };
        Paragraph::new(subtitle)
            .style(Style::default().fg(colors::PURPLE).bg(colors::BG_CARD))
            .alignment(Alignment::Center)
            .render(subtitle_area, buf);

        // ═══════════════════════════════════════════════════════════════
        // MAIN PROGRESS DISPLAY - THE STAR OF THE SHOW
        // ═══════════════════════════════════════════════════════════════

        let progress_area = chunks[1];

        // Background card
        let card_bg = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(colors::BG_CARD).bg(colors::BG))
            .border_type(BorderType::Thick);
        card_bg.render(progress_area, buf);

        let inner = ratatui::prelude::Rect {
            x: progress_area.x + 2,
            y: progress_area.y + 1,
            width: progress_area.width - 4,
            height: progress_area.height - 2,
        };

        // Calculate bar width - make it 80% of available space
        let bar_width = ((inner.width as f64 * 0.8) as usize).max(20);
        let bar_start = inner.x + (inner.width - bar_width as u16) / 2;

        let mut y_offset = 1;

        // Render each timer's big progress bar
        for state in self.states.iter() {
            let rem = state.end_time - self.now;
            let expired = rem.num_milliseconds() <= 0;

            let progress = if expired {
                1.0
            } else {
                1.0 - (rem.num_milliseconds() as f64 / (state.total.as_millis() as f64))
            };

            let time_str = if expired {
                "━━━ COMPLETE ━━━".to_string()
            } else {
                format_time_remaining(rem.num_seconds())
            };

            let (bar, bar_color) = create_animated_bar(progress, bar_width, self.elapsed_ms);

            let label_text = if expired {
                format!("  ✓ {}  ", state.label.to_uppercase())
            } else {
                format!("  ◉ {}  ", state.label)
            };

            // Row 1: Label
            let label_area = ratatui::prelude::Rect {
                x: inner.x + 2,
                y: inner.y + y_offset,
                width: inner.width - 4,
                height: 1,
            };
            Paragraph::new(label_text)
                .style(Style::default().fg(colors::TEXT).bold())
                .alignment(Alignment::Center)
                .render(label_area, buf);
            y_offset += 1;

            // Row 2: THE PROGRESS BAR
            let bar_area = ratatui::prelude::Rect {
                x: bar_start,
                y: inner.y + y_offset,
                width: bar_width as u16,
                height: 2,
            };

            // Top half of bar
            let bar_area = ratatui::prelude::Rect {
                x: bar_start,
                y: inner.y + y_offset,
                width: bar_width as u16,
                height: 2,
            };

            let bar_string = bar.clone();
            let top_bar = Paragraph::new(bar_string)
                .style(Style::default().fg(bar_color).bg(colors::BG))
                .alignment(Alignment::Left);
            top_bar.render(bar_area, buf);
            y_offset += 1;

            // Bottom half with percentage
            let pct = (progress * 100.0) as u32;
            let pct_text = format!(
                "{:>3}% {} {} ",
                pct,
                "═".repeat(bar_width.saturating_sub(6)),
                time_str
            );
            let pct_string = pct_text.clone();
            let pct_area = ratatui::prelude::Rect {
                x: bar_start,
                y: inner.y + y_offset,
                width: bar_width as u16,
                height: 1,
            };
            Paragraph::new(pct_string)
                .style(Style::default().fg(colors::TEXT_DIM))
                .alignment(Alignment::Center)
                .render(pct_area, buf);
            y_offset += 2;

            // Visual separator between timers
            if y_offset < inner.height - 1 {
                let sep = "─".repeat(((inner.width - 4) as usize).min(40));
                let sep_area = ratatui::prelude::Rect {
                    x: inner.x + 2,
                    y: inner.y + y_offset,
                    width: inner.width - 4,
                    height: 1,
                };
                let sep_string = sep.clone();
                Paragraph::new(sep_string)
                    .style(Style::default().fg(colors::BG_CARD))
                    .alignment(Alignment::Center)
                    .render(sep_area, buf);
                y_offset += 1;
            }
        }

        // ═══════════════════════════════════════════════════════════════
        // TIMER LIST
        // ═══════════════════════════════════════════════════════════════
        let mut rows = Vec::new();
        for state in self.states.iter() {
            let rem = state.end_time - self.now;
            let expired = rem.num_milliseconds() <= 0;
            let progress = if expired {
                1.0
            } else {
                1.0 - (rem.num_milliseconds() as f64 / (state.total.as_millis() as f64))
            };

            let color = if expired {
                colors::PINK_NEON
            } else if progress > 0.75 {
                colors::GREEN
            } else if progress > 0.5 {
                colors::CYAN
            } else if progress > 0.25 {
                colors::YELLOW
            } else {
                colors::RED
            };

            let icon = if expired { "✓" } else { "◈" };
            let time_str = if expired {
                "DONE".to_string()
            } else {
                format_time_remaining(rem.num_seconds())
            };

            let row = Row::new([format!("{}  {}", icon, state.label), time_str])
                .style(Style::default().fg(color))
                .height(1);
            rows.push(row);
        }

        let widths = [Constraint::Percentage(70), Constraint::Percentage(30)];
        let table = Table::new(rows, widths)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(colors::BG_CARD))
                    .title("  TIMERS  ")
                    .title_style(Style::default().fg(colors::CYAN).bold()),
            )
            .column_spacing(1);
        table.render(chunks[2], buf);

        // ═══════════════════════════════════════════════════════════════
        // FOOTER
        // ═══════════════════════════════════════════════════════════════
        let active_count = self
            .states
            .iter()
            .filter(|s| (s.end_time - self.now).num_milliseconds() > 0)
            .count();
        let completed_count = self.states.len() - active_count;

        let footer_text = if active_count == 0 && !self.states.is_empty() {
            "  ✨ ★ ALL DONE! ★ ✨   Press [q] or [Enter] to exit   ✨ ★ ".to_string()
        } else {
            format!(
                "  ⏸ {:2} active  ✓ {:2} complete  │  [Ctrl+C] or [q] to quit  ",
                active_count, completed_count
            )
        };

        let footer_color = if active_count == 0 && !self.states.is_empty() {
            colors::GREEN
        } else {
            colors::TEXT_DIM
        };

        let footer = Paragraph::new(footer_text)
            .style(Style::default().fg(footer_color).bg(colors::BG_CARD))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(border_style)
                    .border_type(BorderType::Thick),
            );
        footer.render(chunks[3], buf);
    }
}

fn main() -> io::Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        println!("🎮 CyberTimer v0.1.0");
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("Usage: cyber-timer 30s 5m 'work:25m'");
        println!();
        println!("Examples:");
        println!("  cyber-timer 30s          # 30 second timer");
        println!("  cyber-timer 5m 2m       # 5 min, then 2 min");
        println!("  cyber-timer 'work:25m'  # named timer");
        println!("  cyber-timer 25m 5m 25m  # pomodoro!");
        println!();
        println!("Controls:");
        println!("  Enter / q  →  Exit when complete");
        println!("  Ctrl+C     →  Exit immediately");
        return Ok(());
    }

    let mut states: Vec<TimerState> = args
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let (label, d) = parse_labeled_duration(s);
            TimerState {
                label: if i == 0 {
                    label
                } else if label == "Timer" {
                    format!("Task {}", i + 1)
                } else {
                    label
                },
                total: d,
                end_time: Local::now() + ChronoDuration::from_std(d).unwrap(),
                alert_triggered: false,
            }
        })
        .collect();

    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    enable_raw_mode()?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let running_for_thread = running.clone();
    let tick_rate = Duration::from_millis(80);

    thread::spawn(move || {
        while running_for_thread.load(std::sync::atomic::Ordering::Relaxed) {
            if event::poll(Duration::from_millis(10)).unwrap_or(false) {
                if let Ok(Event::Key(key)) = event::read() {
                    if key.kind == KeyEventKind::Press {
                        if key.code == KeyCode::Char('q') || key.code == KeyCode::Enter {
                            running_for_thread.store(false, std::sync::atomic::Ordering::Relaxed);
                        }
                        if key.code == KeyCode::Char('c')
                            && key.modifiers.contains(event::KeyModifiers::CONTROL)
                        {
                            running_for_thread.store(false, std::sync::atomic::Ordering::Relaxed);
                        }
                    }
                }
            }
        }
    });

    let start = Instant::now();

    loop {
        if !running.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }

        let now = Local::now();
        let elapsed_ms = start.elapsed().as_millis() as u64;

        terminal.draw(|f| {
            let size = f.size();
            let widget = TimerWidget::new(&states, now, elapsed_ms);
            f.render_widget(widget, size);
        })?;

        // Check for completed timers and trigger alerts
        for (i, state) in states.iter_mut().enumerate() {
            let rem = state.end_time - now;
            if rem.num_milliseconds() <= 0 && !state.alert_triggered {
                state.alert_triggered = true;
                let label = state.label.clone();
                thread::spawn(move || {
                    do_alert(&label);
                });
            }
        }

        thread::sleep(tick_rate);

        let active_found = states
            .iter()
            .any(|s| (s.end_time - now).num_milliseconds() > 0);

        if !active_found && start.elapsed() > Duration::from_millis(500) {
            if event::poll(Duration::from_millis(10)).unwrap_or(false) {
                if let Ok(Event::Key(key)) = event::read() {
                    if key.kind == KeyEventKind::Press {
                        if key.code == KeyCode::Char('q') || key.code == KeyCode::Enter {
                            break;
                        }
                    }
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}
