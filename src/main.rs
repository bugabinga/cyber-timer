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
    prelude::Stylize,
    style::Color,
    style::Style,
    widgets::{Block, BorderType, Borders, Paragraph, Widget},
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
        .args(["-t", "6000", "-i", "alarm-clock", "🎯 Timer Done!", label])
        .spawn();
    let _ = Command::new("pw-play").arg(FALLBACK_SOUND).spawn();
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

    let phase = (elapsed_ms / 150) % 4;

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

struct TimerWidget<'a> {
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

        let footer_text = if active == 0 && !self.states.is_empty() {
            "  ✨ ALL COMPLETE! Press [q] or [Enter] to exit  "
        } else {
            &format!(
                "  ▶ {:2} active  ✓ {:2} done  │  [Ctrl+C] or [q] to quit  ",
                active, done
            )
        };

        let footer_color = if active == 0 && !self.states.is_empty() {
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

fn cleanup_terminal() {
    // Try to restore terminal state - ignore errors
    let _ = disable_raw_mode();
    let _ = execute!(std::io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
    let _ = std::io::Write::write_all(&mut std::io::stdout(), b"\x1b[?1000l");
    let _ = std::io::Write::write_all(&mut std::io::stdout(), b"\x1b[?1002l");
    let _ = std::io::Write::write_all(&mut std::io::stdout(), b"\x1b[?1049l");
}

fn main() {
    // Set up panic hook to always clean up terminal
    std::panic::set_hook(Box::new(|_| {
        cleanup_terminal();
        eprintln!("\nPanic occurred, terminal cleaned up.");
    }));

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
        println!("Controls: Enter/q = exit, Ctrl+C = force quit");
        return;
    }

    // Limit number of timers
    let args: Vec<String> = args.into_iter().take(10).collect();

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

    let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let running_clone = running.clone();

    // Input handling thread
    let input_thread = thread::spawn(move || {
        while running_clone.load(std::sync::atomic::Ordering::Relaxed) {
            if event::poll(Duration::from_millis(50)).unwrap_or(false) {
                if let Ok(Event::Key(key)) = event::read() {
                    if key.kind == KeyEventKind::Press {
                        if key.code == KeyCode::Char('q') || key.code == KeyCode::Enter {
                            running_clone.store(false, std::sync::atomic::Ordering::Relaxed);
                        }
                        if key.code == KeyCode::Char('c')
                            && key.modifiers.contains(event::KeyModifiers::CONTROL)
                        {
                            running_clone.store(false, std::sync::atomic::Ordering::Relaxed);
                        }
                    }
                }
            }
        }
    });

    let start = Instant::now();
    let mut last_state = Vec::new();

    let result = (|| -> io::Result<()> {
        loop {
            if !running.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }

            let now = Local::now();
            let elapsed_ms = start.elapsed().as_millis() as u64;

            terminal.draw(|f| {
                f.render_widget(
                    TimerWidget {
                        states: &states,
                        now,
                        elapsed_ms,
                    },
                    f.size(),
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

            thread::sleep(Duration::from_millis(80));

            // Check if all done
            let active = states
                .iter()
                .any(|s| (s.end_time - now).num_milliseconds() > 0);
            if !active && start.elapsed() > Duration::from_millis(500) {
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

            last_state = states.clone();
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
