//! Buddy sprite — animated companion character rendered in the TUI sidebar.
//!
//! The `Dog` species animates differently for each query-loop stage so the
//! user can see at a glance what the agent is doing:
//!
//!   Idle      → dog sitting, tail wagging left/right
//!   Thinking  → dog with raised paw and quizzical look
//!   Working   → dog running (legs alternate)
//!   Happy     → dog bouncing with excitement
//!   Error     → dog with X eyes, head down
//!   Sleeping  → dog napping with Z's

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Widget};
use serde::{Deserialize, Serialize};

use super::theme::Theme;

// ─── Species ────────────────────────────────────────────────────────────────

/// Available buddy species.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BuddySpecies {
    Fox,
    Cat,
    Owl,
    Robot,
    Dragon,
    Ghost,
    Dog,
}

// ─── Rarity ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuddyRarity {
    Common,
    Uncommon,
    Rare,
    Legendary,
}

// ─── Mood ────────────────────────────────────────────────────────────────────

/// Mood driven by query-loop stage.
#[derive(Debug, Clone, PartialEq)]
pub enum BuddyMood {
    Idle,
    Thinking,
    Working,
    Happy,
    Error,
    Sleeping,
}

// ─── Buddy state ─────────────────────────────────────────────────────────────

/// The full buddy sprite state carried by `App`.
#[derive(Debug, Clone)]
pub struct Buddy {
    pub species:       BuddySpecies,
    pub rarity:        BuddyRarity,
    pub mood:          BuddyMood,
    pub name:          String,
    pub enabled:       bool,
    /// One-line label set by the query loop (e.g. "Running: Grep").
    /// Shown below the sprite; overrides the mood's default label when set.
    pub stage_message: String,
    pub(super) frame:  usize,
}

impl Buddy {
    pub fn new() -> Self {
        Buddy {
            species:       BuddySpecies::Dog,
            rarity:        BuddyRarity::Common,
            mood:          BuddyMood::Idle,
            name:          "Carimi".to_string(),
            enabled:       true,
            stage_message: String::new(),
            frame:         0,
        }
    }

    /// Advance the animation frame (called on every `AppEvent::Tick`).
    pub fn tick(&mut self) {
        self.frame = (self.frame + 1) % 8;
    }

    /// Default stage label derived from the current mood.
    pub fn default_stage_label(&self) -> &str {
        match self.mood {
            BuddyMood::Idle     => "Ready",
            BuddyMood::Thinking => "Thinking...",
            BuddyMood::Working  => "Working...",
            BuddyMood::Happy    => "Done!",
            BuddyMood::Error    => "Error!",
            BuddyMood::Sleeping => "Sleeping...",
        }
    }

    /// Stage label to display — prefers `stage_message` when non-empty.
    pub fn stage_label(&self) -> &str {
        if self.stage_message.is_empty() {
            self.default_stage_label()
        } else {
            &self.stage_message
        }
    }

    /// Border colour keyed on rarity.
    pub fn rarity_color(&self) -> Color {
        match self.rarity {
            BuddyRarity::Common    => Color::White,
            BuddyRarity::Uncommon  => Color::Green,
            BuddyRarity::Rare      => Color::Blue,
            BuddyRarity::Legendary => Color::Magenta,
        }
    }

    /// ASCII-art frames for the current species × mood × animation frame.
    ///
    /// Dog art is the user-supplied side-profile design, 11–12 lines tall.
    /// Lines that vary by mood:
    ///   line 2  — eyes   (●  ?  ^  ★  ×  -)
    ///   line 4  — mouth  (╰──╯  or  ╭──╮  for sad)
    ///   line 6  — tail   (▄▄▄ low  /  ▀▀▀ raised, animated for Idle)
    ///   line 11 — Z's    (Sleeping only, 12-line variant)
    pub fn art(&self) -> &[&'static str] {
        match (&self.species, &self.mood) {

            // ── Dog (OpenClaw) ─────────────────────────────────────────────

            (BuddySpecies::Dog, BuddyMood::Idle) => {
                // Tail wags: ▄▄▄ (frame 0-3) → ▀▀▀ raised (frame 4-7).
                if self.frame < 4 {
                    &[
                        "▄▄      ▄▄        ",
                        "   ██▄▄▄▄▄▄██     ",
                        "   █ ●    ● █     ",
                        "   █    ▼    █    ",
                        "    █ ╰──╯  █     ",
                        "     ▀█▄▄▄█▀      ",
                        "      █    █▄▄▄▄▄▄▄",
                        "      █              ▀╲",
                        "      █               █",
                        "       ██          ██▄▀",
                        "        ▀▀        ▀▀   ",
                    ]
                } else {
                    &[
                        "▄▄      ▄▄        ",
                        "   ██▄▄▄▄▄▄██     ",
                        "   █ ●    ● █     ",
                        "   █    ▼    █    ",
                        "    █ ╰──╯  █     ",
                        "     ▀█▄▄▄█▀      ",
                        "      █    █▀▀▀▀▀▀▀",  // tail raised
                        "      █              ▀╲",
                        "      █               █",
                        "       ██          ██▄▀",
                        "        ▀▀        ▀▀   ",
                    ]
                }
            }

            (BuddySpecies::Dog, BuddyMood::Thinking) => {
                // Eyes alternate ? and . (pondering blink)
                match self.frame % 2 {
                    0 => &[
                        "▄▄      ▄▄        ",
                        "   ██▄▄▄▄▄▄██     ",
                        "   █ ?    ? █     ",
                        "   █    ▼    █    ",
                        "    █ ╰──╯  █     ",
                        "     ▀█▄▄▄█▀      ",
                        "      █    █▄▄▄▄▄▄▄",
                        "      █              ▀╲",
                        "      █               █",
                        "       ██          ██▄▀",
                        "        ▀▀        ▀▀   ",
                    ],
                    _ => &[
                        "▄▄      ▄▄        ",
                        "   ██▄▄▄▄▄▄██     ",
                        "   █ .    . █     ",
                        "   █    ▼    █    ",
                        "    █ ╰──╯  █     ",
                        "     ▀█▄▄▄█▀      ",
                        "      █    █▄▄▄▄▄▄▄",
                        "      █              ▀╲",
                        "      █               █",
                        "       ██          ██▄▀",
                        "        ▀▀        ▀▀   ",
                    ],
                }
            }

            (BuddySpecies::Dog, BuddyMood::Working) => {
                // ^ eyes; legs alternate between two stride positions.
                match self.frame % 2 {
                    0 => &[
                        "▄▄      ▄▄        ",
                        "   ██▄▄▄▄▄▄██     ",
                        "   █ ^    ^ █     ",
                        "   █    ▼    █    ",
                        "    █ ╰──╯  █     ",
                        "     ▀█▄▄▄█▀      ",
                        "      █    █▄▄▄▄▄▄▄",
                        "      █              ▀╲",
                        "      █               █",
                        "       ██          ██   ",  // stride A
                        "       /▀▀        ▀▀    ",
                    ],
                    _ => &[
                        "▄▄      ▄▄        ",
                        "   ██▄▄▄▄▄▄██     ",
                        "   █ ^    ^ █     ",
                        "   █    ▼    █    ",
                        "    █ ╰──╯  █     ",
                        "     ▀█▄▄▄█▀      ",
                        "      █    █▄▄▄▄▄▄▄",
                        "      █              ▀╲",
                        "      █               █",
                        "       ██          ██   ",  // stride B
                        "        ▀▀▀       ▀▀    ",
                    ],
                }
            }

            (BuddySpecies::Dog, BuddyMood::Happy) => {
                // ★ eyes + tail raised; body bounces on odd frames.
                match self.frame % 4 {
                    0 | 2 => &[
                        "▄▄      ▄▄        ",
                        "   ██▄▄▄▄▄▄██     ",
                        "   █ ★    ★ █     ",
                        "   █    ▼    █    ",
                        "    █ ╰──╯  █     ",
                        "     ▀█▄▄▄█▀      ",
                        "      █    █▀▀▀▀▀▀▀",
                        "      █              ▀╲",
                        "      █               █",
                        "       ██          ██▄▀",
                        "        ▀▀        ▀▀   ",
                    ],
                    _ => &[
                        "                        ",  // body bounced up
                        "▄▄      ▄▄        ",
                        "   ██▄▄▄▄▄▄██     ",
                        "   █ ★    ★ █     ",
                        "   █    ▼    █    ",
                        "    █ ╰──╯  █     ",
                        "     ▀█▄▄▄█▀      ",
                        "      █    █▀▀▀▀▀▀▀",
                        "      █              ▀╲",
                        "      █               █",
                        "       ██          ██▄▀",
                    ],
                }
            }

            (BuddySpecies::Dog, BuddyMood::Error) => {
                // × eyes + sad mouth ╭──╮; slight head-tilt shake.
                match self.frame % 2 {
                    0 => &[
                        "▄▄      ▄▄        ",
                        "   ██▄▄▄▄▄▄██     ",
                        "   █ ×    × █     ",
                        "   █    ▼    █    ",
                        "    █ ╭──╮  █     ",  // sad mouth
                        "     ▀█▄▄▄█▀      ",
                        "      █    █▄▄▄▄▄▄▄",
                        "      █              ▀╲",
                        "      █               █",
                        "       ██          ██▄▀",
                        "        ▀▀        ▀▀   ",
                    ],
                    _ => &[
                        " ▄▄      ▄▄       ",  // head tilted
                        "    ██▄▄▄▄▄▄██    ",
                        "    █ ×    × █    ",
                        "    █    ▼    █   ",
                        "     █ ╭──╮  █   ",
                        "      ▀█▄▄▄█▀    ",
                        "      █    █▄▄▄▄▄▄▄",
                        "      █              ▀╲",
                        "      █               █",
                        "       ██          ██▄▀",
                        "        ▀▀        ▀▀   ",
                    ],
                }
            }

            (BuddySpecies::Dog, BuddyMood::Sleeping) => {
                // - eyes; Z's rise across frames as a 12th art line.
                match self.frame % 4 {
                    0 => &[
                        "▄▄      ▄▄        ",
                        "   ██▄▄▄▄▄▄██     ",
                        "   █ -    - █     ",
                        "   █    ▼    █    ",
                        "    █ ╰──╯  █     ",
                        "     ▀█▄▄▄█▀      ",
                        "      █    █▄▄▄▄▄▄▄",
                        "      █              ▀╲",
                        "      █               █",
                        "       ██          ██▄▀",
                        "        ▀▀        ▀▀   ",
                        "                    z  ",
                    ],
                    1 => &[
                        "▄▄      ▄▄        ",
                        "   ██▄▄▄▄▄▄██     ",
                        "   █ -    - █     ",
                        "   █    ▼    █    ",
                        "    █ ╰──╯  █     ",
                        "     ▀█▄▄▄█▀      ",
                        "      █    █▄▄▄▄▄▄▄",
                        "      █              ▀╲",
                        "      █               █",
                        "       ██          ██▄▀",
                        "        ▀▀        ▀▀   ",
                        "                   Z   ",
                    ],
                    2 => &[
                        "▄▄      ▄▄        ",
                        "   ██▄▄▄▄▄▄██     ",
                        "   █ -    - █     ",
                        "   █    ▼    █    ",
                        "    █ ╰──╯  █     ",
                        "     ▀█▄▄▄█▀      ",
                        "      █    █▄▄▄▄▄▄▄",
                        "      █              ▀╲",
                        "      █               █",
                        "       ██          ██▄▀",
                        "        ▀▀        ▀▀   ",
                        "                  z Z  ",
                    ],
                    _ => &[
                        "▄▄      ▄▄        ",
                        "   ██▄▄▄▄▄▄██     ",
                        "   █ -    - █     ",
                        "   █    ▼    █    ",
                        "    █ ╰──╯  █     ",
                        "     ▀█▄▄▄█▀      ",
                        "      █    █▄▄▄▄▄▄▄",
                        "      █              ▀╲",
                        "      █               █",
                        "       ██          ██▄▀",
                        "        ▀▀        ▀▀   ",
                        "                z Z Z  ",
                    ],
                }
            }

            // ── Fox (original art) ──────────────────────────────────────────
            (BuddySpecies::Fox, BuddyMood::Idle) => match self.frame % 2 {
                0 => &[" /\\_/\\ ", "( o.o )", " > ^ < "],
                _ => &[" /\\_/\\ ", "( o.o )", "  > <  "],
            },
            (BuddySpecies::Fox, BuddyMood::Thinking) => &[" /\\_/\\ ", "( ?.? )", " > ^ < "],
            (BuddySpecies::Fox, BuddyMood::Working) => match self.frame % 2 {
                0 => &[" /\\_/\\ ", "( ^.^ )", " />~<\\ "],
                _ => &[" /\\_/\\ ", "( ^.^ )", " \\>~</ "],
            },
            (BuddySpecies::Fox, BuddyMood::Happy)    => &[" /\\_/\\ ", "( ^w^ )", " > ^ < "],
            (BuddySpecies::Fox, BuddyMood::Error)    => &[" /\\_/\\ ", "( x.x )", " > ^ < "],
            (BuddySpecies::Fox, BuddyMood::Sleeping) => &[" /\\_/\\ ", "( -.- )", " > ^ < zzZ"],

            // ── Other species (original art, mood-agnostic) ─────────────────
            (BuddySpecies::Cat,    _) => &["  /\\_/\\  ", " ( o.o ) ", "  > ^ <  "],
            (BuddySpecies::Owl,    _) => &[" {o,o} ", " /)_)  ", "  \" \"  "],
            (BuddySpecies::Robot,  _) => &[" [===] ", " |o o| ", " |___| "],
            (BuddySpecies::Dragon, _) => &["  /\\_  ", " (o.o> ", " /|  |\\ "],
            (BuddySpecies::Ghost,  _) => &["  ___  ", " (o o) ", " /vvv\\ "],
        }
    }
}

impl Default for Buddy {
    fn default() -> Self { Self::new() }
}

// ─── Widget ──────────────────────────────────────────────────────────────────

/// Renders the buddy in a bordered panel with name + stage label.
pub struct BuddyWidget<'a> {
    buddy: &'a Buddy,
    theme: &'a Theme,
}

impl<'a> BuddyWidget<'a> {
    pub fn new(buddy: &'a Buddy, theme: &'a Theme) -> Self {
        BuddyWidget { buddy, theme }
    }
}

impl<'a> Widget for BuddyWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if !self.buddy.enabled || area.height < 13 || area.width < 25 {
            return;
        }

        let border_color = self.buddy.rarity_color();
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(format!(" {} ", self.buddy.name));

        let inner = block.inner(area);
        block.render(area, buf);

        // ── Draw ASCII art (horizontally centred) ─────────────────────────
        let art = self.buddy.art();
        let art_rows = art.len() as u16;

        // Find the widest art line so we can centre the whole sprite.
        let art_max_w = art.iter()
            .map(|l| l.chars().count())
            .max()
            .unwrap_or(0) as u16;
        let x_off = (inner.width.saturating_sub(art_max_w)) / 2;

        // Dog is always green; other species keep mood-keyed colour.
        let art_color = if self.buddy.species == BuddySpecies::Dog {
            Color::Green
        } else {
            match self.buddy.mood {
                BuddyMood::Idle     => Color::White,
                BuddyMood::Thinking => Color::Yellow,
                BuddyMood::Working  => Color::Cyan,
                BuddyMood::Happy    => Color::Green,
                BuddyMood::Error    => Color::Red,
                BuddyMood::Sleeping => Color::DarkGray,
            }
        };

        for (i, line) in art.iter().enumerate() {
            let y = inner.y + i as u16;
            if y >= inner.y + inner.height { break; }
            buf.set_string(inner.x + x_off, y, line, Style::default().fg(art_color));
        }

        // ── Stage label (centred) ─────────────────────────────────────────
        let label_y = inner.y + art_rows + 1;
        if label_y < inner.y + inner.height {
            let label = self.buddy.stage_label();
            let max_w = inner.width as usize;
            let display: String = label.chars().take(max_w).collect();
            let label_style = match self.buddy.mood {
                BuddyMood::Happy    => Style::default().fg(Color::Green).bold(),
                BuddyMood::Error    => Style::default().fg(Color::Red).bold(),
                BuddyMood::Thinking => Style::default().fg(Color::Yellow),
                BuddyMood::Working  => Style::default().fg(Color::Cyan),
                _                   => self.theme.dim_style(),
            };
            let label_area = Rect::new(inner.x, label_y, inner.width, 1);
            Paragraph::new(display)
                .style(label_style)
                .alignment(Alignment::Center)
                .render(label_area, buf);
        }

        // ── Mood indicator dot ────────────────────────────────────────────
        // A single coloured dot at the bottom-right of the panel border.
        if area.height > 2 && area.width > 2 {
            let dot_y = area.y + area.height - 1;
            let dot_x = area.x + area.width - 2;
            let (dot_char, dot_color) = match self.buddy.mood {
                BuddyMood::Idle     => ("●", Color::White),
                BuddyMood::Thinking => ("●", Color::Yellow),
                BuddyMood::Working  => ("●", Color::Cyan),
                BuddyMood::Happy    => ("●", Color::Green),
                BuddyMood::Error    => ("●", Color::Red),
                BuddyMood::Sleeping => ("●", Color::DarkGray),
            };
            buf.set_string(dot_x, dot_y, dot_char, Style::default().fg(dot_color));
        }
    }
}
