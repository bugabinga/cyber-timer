use chrono::{DateTime, Duration as ChronoDuration, Local};
use clap::Parser;
use crossterm::{
    event::DisableMouseCapture,
    event::EnableMouseCapture,
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use notify_rust::Notification;
use ratatui::{
    backend::CrosstermBackend,
    buffer::Buffer,
    layout::{Alignment, Constraint, Direction, Layout},
    prelude::Stylize,
    style::Color,
    style::Style,
    widgets::{Block, BorderType, Borders, Paragraph, Widget},
    Terminal,
};
use regex::Regex;
use rodio::{Decoder, OutputStream, Source};
use std::{
    io,
    io::Cursor,
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};

#[cfg(not(debug_assertions))]
use std::sync::Arc;
#[cfg(debug_assertions)]
use std::sync::{atomic::AtomicU16, Arc, Mutex};

pub mod colors {
    use ratatui::style::Color;
    pub const BG: Color = Color::Rgb(8, 8, 15);
    pub const BORDER: Color = Color::Rgb(255, 0, 128);
    pub const PINK_NEON: Color = Color::Rgb(255, 0, 255);
    pub const CYAN: Color = Color::Rgb(0, 255, 255);
    pub const GREEN: Color = Color::Rgb(0, 255, 127);
    pub const YELLOW: Color = Color::Rgb(255, 215, 0);
    pub const ORANGE: Color = Color::Rgb(255, 140, 0);
    pub const TEXT: Color = Color::Rgb(200, 200, 220);
    pub const TEXT_DIM: Color = Color::Rgb(100, 100, 130);
    #[cfg(debug_assertions)]
    pub const MATRIX_GREEN: Color = Color::Rgb(0, 255, 65);
}

const FALLBACK_SOUND: &[u8] = include_bytes!("../sounds/complete.oga");

const SLEEP_MS: u64 = 80;
const POLL_MS: u64 = 50;
const ANIMATION_PHASE_MS: u64 = 150;
const EXIT_DELAY_MS: u64 = 500;
#[cfg(debug_assertions)]
const DIAGNOSTIC_FLICKER_MS: u64 = 200;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Timer durations (e.g., 30s, 5m, 'work:25m')
    #[arg(name = "TIMERS", required = true)]
    timers: Vec<String>,
}

static DURATION_REGEX: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();

fn get_duration_regex() -> &'static Regex {
    DURATION_REGEX.get_or_init(|| Regex::new(r"(?i)(\d+)(sec|s|min|m|h)").unwrap())
}

#[derive(Clone, Debug)]
pub struct TimerState {
    label: String,
    total: Duration,
    end_time: DateTime<Local>,
    alert_triggered: bool,
}

#[cfg(debug_assertions)]
struct DiagnosticInfo {
    pid: u32,
    resolution: (u16, u16),
    latency_ms: u64,
    last_key: Option<String>,
    states: Vec<TimerState>,
    scroll_offset: u16,
}

fn parse_duration(s: &str) -> Duration {
    let re = get_duration_regex();
    if let Some(caps) = re.captures(s.trim()) {
        let val: u64 = caps[1].parse().unwrap_or(10);
        match caps[2].to_lowercase().as_str() {
            "min" | "m" => Duration::from_secs(val * 60),
            "h" => Duration::from_secs(val * 3600),
            "sec" | "s" => Duration::from_secs(val),
            _ => Duration::from_secs(val),
        }
    } else {
        Duration::from_secs(10)
    }
}

fn parse_labeled_duration(s: &str) -> (String, Duration) {
    if let Some((label, duration_str)) = s.split_once(':') {
        let trimmed_label = label.trim();
        (
            if trimmed_label.is_empty() {
                "Timer".to_string()
            } else {
                trimmed_label.to_string()
            },
            parse_duration(duration_str),
        )
    } else {
        ("Timer".to_string(), parse_duration(s))
    }
}

fn parse_timers(args: &[String]) -> Vec<TimerState> {
    args.iter()
        .take(10) // Limit number of timers
        .enumerate()
        .map(|(i, s)| {
            let (label, d) = parse_labeled_duration(s);
            let end_time = Local::now()
                + ChronoDuration::from_std(d).unwrap_or_else(|_| ChronoDuration::days(365 * 10));
            TimerState {
                label: if i > 0 && label == "Timer" {
                    format!("Task {}", i + 1)
                } else {
                    label
                },
                total: d,
                end_time,
                alert_triggered: false,
            }
        })
        .collect()
}

fn do_alert(label: &str) {
    if let Err(e) = Notification::new()
        .summary("Timer Done!")
        .body(label)
        .icon("alarm-clock")
        .timeout(6000)
        .show()
    {
        log::warn!("Failed to send notification: {}", e);
    }

    let sound_data = FALLBACK_SOUND.to_vec();
    thread::spawn(move || {
        if let Ok((_stream, stream_handle)) = OutputStream::try_default() {
            if let Ok(source) = Decoder::new(Cursor::new(sound_data)) {
                let _ = stream_handle.play_raw(source.convert_samples());
                thread::sleep(Duration::from_secs(2));
            }
        }
    });
}

fn get_bar_color(progress: f64) -> Color {
    if progress >= 1.0 {
        colors::PINK_NEON
    } else if progress > 0.75 {
        colors::GREEN
    } else if progress > 0.5 {
        colors::CYAN
    } else if progress > 0.25 {
        colors::YELLOW
    } else {
        colors::ORANGE
    }
}

fn format_time(secs: i64) -> String {
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

fn create_bar(progress: f64, width: usize, elapsed_ms: u64) -> String {
    let filled = ((progress * width as f64) as usize).min(width);
    let empty = width.saturating_sub(filled);

    let phase = (elapsed_ms / ANIMATION_PHASE_MS) % 4;

    let edge = match phase {
        0 => "▓",
        1 => "▒",
        2 => "░",
        _ => "▒",
    };

    if filled == 0 {
        "░".repeat(width)
    } else if filled >= width {
        "█".repeat(width)
    } else if filled == 1 {
        format!("{}{}", edge, "░".repeat(empty))
    } else {
        format!("{}{}", "█".repeat(filled - 1), edge)
    }
}

pub struct TimerWidget<'a> {
    states: &'a [TimerState],
    now: DateTime<Local>,
    elapsed_ms: u64,
}

impl<'a> Widget for TimerWidget<'a> {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut Buffer) {
        // Need minimum size
        if area.width < 40 || area.height < 5 {
            return;
        }

        // Simple layout: header | progress bars | footer
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(3),
                Constraint::Length(3),
            ])
            .split(area);

        // ═══════════════════════════════════════════
        // HEADER
        // ═══════════════════════════════════════════
        let time_str = self.now.format("%H:%M:%S").to_string();

        let header = Paragraph::new(format!(
            " ⏱  CyberTimer  │  {}  │  ⚡ RUST POWERED  ",
            time_str
        ))
        .style(Style::default().fg(colors::TEXT).bg(colors::BG))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::BORDER))
                .border_type(BorderType::Thick),
        );

        header.render(chunks[0], buf);

        // ═══════════════════════════════════════════
        // MAIN PROGRESS BARS
        // ═══════════════════════════════════════════

        // Guard against too many timers
        let num_timers = self.states.len().min(10);
        if num_timers == 0 {
            return;
        }

        let bar_width = ((chunks[1].width as f64 * 0.6) as usize)
            .max(15)
            .min(chunks[1].width as usize - 25);
        let bar_start = chunks[1].x + (chunks[1].width.saturating_sub(bar_width as u16 + 25)) / 2;

        let border = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(colors::BORDER).bg(colors::BG))
            .border_type(BorderType::Thick)
            .title("  TIMERS  ")
            .title_style(Style::default().fg(colors::CYAN).bold());
        border.render(chunks[1], buf);

        let inner = ratatui::prelude::Rect {
            x: chunks[1].x + 1,
            y: chunks[1].y + 1,
            width: chunks[1].width.saturating_sub(2),
            height: chunks[1].height.saturating_sub(2),
        };

        // Calculate height per timer
        let height_per_timer = inner.height / num_timers as u16;
        if height_per_timer < 1 {
            return;
        }

        let mut y = inner.y;

        for state in self.states.iter().take(num_timers) {
            if state.total.as_millis() == 0 {
                continue;
            }

            let rem = state.end_time - self.now;
            let expired = rem.num_milliseconds() <= 0;

            let progress = if expired {
                1.0
            } else {
                1.0 - (rem.num_milliseconds() as f64 / (state.total.as_millis() as f64))
            };

            let color = get_bar_color(progress);
            let time_str = if expired {
                "DONE".to_string()
            } else {
                format_time(rem.num_seconds())
            };
            let pct = (progress * 100.0) as u32;
            let icon = if expired { "✓" } else { "▶" };

            // Label (max 15 chars)
            let label_text = if state.label.len() > 15 {
                format!("{}...", &state.label[..12])
            } else {
                state.label.clone()
            };

            let label_area = ratatui::prelude::Rect {
                x: inner.x + 2,
                y,
                width: 18,
                height: 1,
            };
            Paragraph::new(format!("{} {}", icon, label_text))
                .style(Style::default().fg(color).bold())
                .render(label_area, buf);

            // Progress bar
            let bar_area = ratatui::prelude::Rect {
                x: bar_start,
                y,
                width: bar_width as u16,
                height: 1,
            };
            let bar = create_bar(progress, bar_width, self.elapsed_ms);
            Paragraph::new(bar)
                .style(Style::default().fg(color).bg(colors::BG))
                .render(bar_area, buf);

            // Percentage + Time
            let pct_area = ratatui::prelude::Rect {
                x: bar_start + bar_width as u16 + 2,
                y,
                width: 20,
                height: 1,
            };
            Paragraph::new(format!("{:>3}% {}", pct, time_str))
                .style(Style::default().fg(colors::TEXT_DIM))
                .render(pct_area, buf);

            y += height_per_timer;
        }

        // ═══════════════════════════════════════════
        // FOOTER
        // ═══════════════════════════════════════════
        let active = self
            .states
            .iter()
            .filter(|s| (s.end_time - self.now).num_milliseconds() > 0)
            .count();
        let done = self.states.len() - active;
        let all_complete = active == 0 && !self.states.is_empty();

        let footer_text = if all_complete {
            "  ✨ ALL COMPLETE! Press [q] or [Enter] to exit  "
        } else {
            &format!(
                "  ▶ {:2} active  ✓ {:2} done  │  [Ctrl+C] or [q] to quit  ",
                active, done
            )
        };

        let footer_color = if all_complete {
            colors::GREEN
        } else {
            colors::TEXT_DIM
        };

        let footer = Paragraph::new(footer_text.to_string())
            .style(Style::default().fg(footer_color).bg(colors::BG))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(colors::BORDER))
                    .border_type(BorderType::Thick),
            );
        footer.render(chunks[2], buf);
    }
}

#[cfg(debug_assertions)]
struct DiagnosticWidget {
    info: DiagnosticInfo,
    elapsed_ms: u64,
}

#[cfg(debug_assertions)]
impl Widget for DiagnosticWidget {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut Buffer) {
        let flicker = (self.elapsed_ms / DIAGNOSTIC_FLICKER_MS).is_multiple_of(2);
        let title = if flicker {
            " [ 💾 SYSTEM_KERNEL_DUMP ] "
        } else {
            " [ ⚡ SYSTEM_KERNEL_DUMP ] "
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .border_style(Style::default().fg(colors::PINK_NEON))
            .title(title)
            .title_style(Style::default().fg(colors::CYAN).bold());

        let inner = block.inner(area);
        block.render(area, buf);

        let debug_text = format!(
            "PID: {}\nRES: {}x{}\nLAT: {}ms\nKEY: {:?}\n\nCORE_STATES:\n{:#?}",
            self.info.pid,
            self.info.resolution.0,
            self.info.resolution.1,
            self.info.latency_ms,
            self.info.last_key.unwrap_or_else(|| "NONE".to_string()),
            self.info.states
        );

        Paragraph::new(debug_text)
            .style(Style::default().fg(colors::MATRIX_GREEN))
            .scroll((self.info.scroll_offset, 0))
            .render(inner, buf);
    }
}

fn cleanup_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(std::io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
    let _ = std::io::Write::write_all(&mut std::io::stdout(), b"\x1b[?1000l\x1b[?1002l\x1b[?1049l");
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    // Set up panic hook to always clean up terminal
    std::panic::set_hook(Box::new(|_| {
        cleanup_terminal();
        eprintln!("\nPanic occurred, terminal cleaned up.");
    }));

    let args = Args::parse();
    let mut states = parse_timers(&args.timers);

    if states.is_empty() {
        return;
    }

    // Set up terminal
    let mut stdout = io::stdout();
    let _ = execute!(stdout, EnterAlternateScreen, EnableMouseCapture);
    let _ = enable_raw_mode();

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = match Terminal::new(backend) {
        Ok(t) => t,
        Err(e) => {
            cleanup_terminal();
            eprintln!("Failed to create terminal: {}", e);
            return;
        }
    };

    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();

    #[cfg(debug_assertions)]
    let debug_enabled = Arc::new(AtomicBool::new(false));
    #[cfg(debug_assertions)]
    let debug_enabled_clone = debug_enabled.clone();
    #[cfg(debug_assertions)]
    let last_key = Arc::new(Mutex::new(None));
    #[cfg(debug_assertions)]
    let last_key_clone = last_key.clone();
    #[cfg(debug_assertions)]
    let scroll_offset = Arc::new(AtomicU16::new(0));
    #[cfg(debug_assertions)]
    let scroll_offset_clone = scroll_offset.clone();

    // Input handling thread
    let input_thread = thread::spawn(move || {
        while running_clone.load(Ordering::Relaxed) {
            if event::poll(Duration::from_millis(POLL_MS)).unwrap_or(false) {
                match event::read() {
                    Ok(Event::Key(key)) => {
                        if key.kind == KeyEventKind::Press {
                            #[cfg(debug_assertions)]
                            {
                                if let KeyCode::Char(c) = key.code {
                                    *last_key_clone.lock().unwrap() = Some(c.to_string());
                                }
                                if key.code == KeyCode::Char('d') {
                                    let val = debug_enabled_clone.load(Ordering::Relaxed);
                                    debug_enabled_clone.store(!val, Ordering::Relaxed);
                                }
                                if debug_enabled_clone.load(Ordering::Relaxed) {
                                    match key.code {
                                        KeyCode::Up => {
                                            scroll_offset_clone
                                                .fetch_update(
                                                    Ordering::Relaxed,
                                                    Ordering::Relaxed,
                                                    |v| {
                                                        if v > 0 {
                                                            Some(v.saturating_sub(3))
                                                        } else {
                                                            None
                                                        }
                                                    },
                                                )
                                                .ok();
                                        }
                                        KeyCode::Down => {
                                            scroll_offset_clone.fetch_add(3, Ordering::Relaxed);
                                        }
                                        _ => {}
                                    }
                                }
                            }

                            if key.code == KeyCode::Char('q') || key.code == KeyCode::Enter {
                                running_clone.store(false, Ordering::Relaxed);
                            }
                            if key.code == KeyCode::Char('c')
                                && key.modifiers.contains(event::KeyModifiers::CONTROL)
                            {
                                running_clone.store(false, Ordering::Relaxed);
                            }
                        }
                    }
                    #[cfg(debug_assertions)]
                    Ok(Event::Mouse(mouse)) => {
                        use crossterm::event::MouseEventKind;
                        if debug_enabled_clone.load(Ordering::Relaxed) {
                            match mouse.kind {
                                MouseEventKind::ScrollUp => {
                                    scroll_offset_clone
                                        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
                                            if v > 0 {
                                                Some(v.saturating_sub(3))
                                            } else {
                                                None
                                            }
                                        })
                                        .ok();
                                }
                                MouseEventKind::ScrollDown => {
                                    scroll_offset_clone.fetch_add(3, Ordering::Relaxed);
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    });

    let start = Instant::now();

    #[cfg(debug_assertions)]
    let mut _last_tick = Instant::now();

    let result = (|| -> io::Result<()> {
        loop {
            if !running.load(Ordering::Relaxed) {
                break;
            }

            let now = Local::now();
            let elapsed_ms = start.elapsed().as_millis() as u64;

            #[cfg(debug_assertions)]
            let tick_latency = _last_tick.elapsed().as_millis() as u64;
            #[cfg(debug_assertions)]
            {
                _last_tick = Instant::now();
            }

            terminal.draw(|f| {
                #[cfg(debug_assertions)]
                let main_area = if debug_enabled.load(Ordering::Relaxed) {
                    let chunks = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
                        .split(f.size());

                    f.render_widget(
                        DiagnosticWidget {
                            info: DiagnosticInfo {
                                pid: std::process::id(),
                                resolution: (f.size().width, f.size().height),
                                latency_ms: tick_latency,
                                last_key: last_key.lock().unwrap().clone(),
                                states: states.clone(),
                                scroll_offset: scroll_offset.load(Ordering::Relaxed),
                            },
                            elapsed_ms,
                        },
                        chunks[1],
                    );
                    chunks[0]
                } else {
                    f.size()
                };

                #[cfg(not(debug_assertions))]
                let main_area = f.size();

                f.render_widget(
                    TimerWidget {
                        states: &states,
                        now,
                        elapsed_ms,
                    },
                    main_area,
                );
            })?;

            // Check for completed timers
            for state in states.iter_mut() {
                let rem = state.end_time - now;
                if rem.num_milliseconds() <= 0 && !state.alert_triggered {
                    state.alert_triggered = true;
                    let label = state.label.clone();
                    thread::spawn(move || do_alert(&label));
                }
            }

            thread::sleep(Duration::from_millis(SLEEP_MS));

            // Check if all done and wait for user input
            let all_done = !states
                .iter()
                .any(|s| (s.end_time - now).num_milliseconds() > 0);
            if all_done && start.elapsed() > Duration::from_millis(EXIT_DELAY_MS) {
                // Keep rendering until user presses q or Enter
                if event::poll(Duration::from_millis(POLL_MS)).unwrap_or(false) {
                    if let Ok(Event::Key(key)) = event::read() {
                        if key.kind == KeyEventKind::Press
                            && (key.code == KeyCode::Char('q') || key.code == KeyCode::Enter)
                        {
                            break;
                        }
                    }
                }
                // Continue looping to keep screen visible
                continue;
            }
        }
        Ok(())
    })();

    // Always clean up terminal
    cleanup_terminal();

    // Wait for input thread
    let _ = input_thread.join();

    if let Err(e) = result {
        eprintln!("Error: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration as ChronoDuration, Local};
    use insta::assert_snapshot;
    use ratatui::{backend::TestBackend, Terminal};
    use std::time::Duration;

    fn make_timer_state(label: &str, total_secs: u64, progress: f64) -> TimerState {
        let total = std::time::Duration::from_secs(total_secs);
        let remaining = (total_secs as f64 * (1.0 - progress)) as i64;
        let end_time = fixed_time() + ChronoDuration::seconds(remaining);
        TimerState {
            label: label.to_string(),
            total,
            end_time,
            alert_triggered: progress >= 1.0,
        }
    }

    fn render_timer(states: &[TimerState], elapsed_ms: u64) -> String {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();

        terminal
            .draw(|f| {
                f.render_widget(
                    TimerWidget {
                        states,
                        now: fixed_time(),
                        elapsed_ms,
                    },
                    f.size(),
                );
            })
            .unwrap();

        terminal.backend().to_string()
    }

    fn fixed_time() -> DateTime<Local> {
        Local::now()
            .date_naive()
            .and_hms_opt(12, 0, 0)
            .unwrap()
            .and_local_timezone(Local)
            .unwrap()
    }

    #[test]
    fn test_single_timer() {
        let states = vec![make_timer_state("Work", 60, 0.5)];
        let terminal = render_timer(&states, 0);
        assert_snapshot!(terminal);
    }

    #[test]
    fn test_multiple_timers() {
        let states = vec![
            make_timer_state("Task 1", 60, 0.25),
            make_timer_state("Task 2", 120, 0.5),
            make_timer_state("Task 3", 30, 0.75),
        ];
        let terminal = render_timer(&states, 0);
        assert_snapshot!(terminal);
    }

    #[test]
    fn test_completed_timer() {
        let states = vec![make_timer_state("Done!", 60, 1.0)];
        let terminal = render_timer(&states, 0);
        assert_snapshot!(terminal);
    }

    #[test]
    fn test_empty_state() {
        let states: Vec<TimerState> = vec![];
        let terminal = render_timer(&states, 0);
        assert_snapshot!(terminal);
    }

    #[test]
    fn test_parse_duration_seconds() {
        assert_eq!(parse_duration("30s"), Duration::from_secs(30));
        assert_eq!(parse_duration("30sec"), Duration::from_secs(30));
    }

    #[test]
    fn test_parse_duration_minutes() {
        assert_eq!(parse_duration("5m"), Duration::from_secs(300));
        assert_eq!(parse_duration("5min"), Duration::from_secs(300));
    }

    #[test]
    fn test_parse_duration_hours() {
        assert_eq!(parse_duration("2h"), Duration::from_secs(7200));
    }

    #[test]
    fn test_parse_duration_invalid() {
        assert_eq!(parse_duration("invalid"), Duration::from_secs(10));
    }

    #[test]
    fn test_parse_labeled_duration() {
        let (label, dur) = parse_labeled_duration("work:25m");
        assert_eq!(label, "work");
        assert_eq!(dur, Duration::from_secs(1500));
    }

    #[test]
    fn test_parse_labeled_duration_no_label() {
        let (label, dur) = parse_labeled_duration("30s");
        assert_eq!(label, "Timer");
        assert_eq!(dur, Duration::from_secs(30));
    }

    #[test]
    fn test_zero_duration_timer() {
        let states = vec![make_timer_state("Zero", 0, 0.0)];
        let terminal = render_timer(&states, 0);
        assert_snapshot!(terminal);
    }

    #[test]
    fn test_format_time() {
        assert_eq!(format_time(0), "00:00");
        assert_eq!(format_time(30), "00:30");
        assert_eq!(format_time(60), "01:00");
        assert_eq!(format_time(90), "01:30");
        assert_eq!(format_time(3600), "01:00:00");
        assert_eq!(format_time(3661), "01:01:01");
    }

    #[test]
    fn test_parse_duration_performance() {
        let inputs = ["30s", "5m", "2h", "invalid", "work:25m"];

        let start = Instant::now();
        for _ in 0..10000 {
            for input in &inputs {
                parse_duration(input);
                parse_labeled_duration(input);
            }
        }
        let elapsed = start.elapsed();

        let ns_per_call = elapsed.as_nanos() / (inputs.len() * 10000) as u128;
        assert!(
            ns_per_call < 10000,
            "parse_duration too slow: {}ns/call",
            ns_per_call
        );
    }
}
