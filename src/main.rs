use std::cmp::{max, min};
use std::collections::BTreeMap;
use unicode_width::UnicodeWidthStr;
use zellij_tile::prelude::*;

// ========== COLOR SYSTEM ==========

/// Color specification supporting default, 256-color, and RGB
#[derive(Debug, Clone, Copy, PartialEq, Default)]
enum ColorSpec {
    /// Use terminal default color
    #[default]
    Default,
    /// 256-color palette index (0-255)
    EightBit(u8),
    /// True color RGB
    Rgb(u8, u8, u8),
}

impl ColorSpec {
    /// Generate ANSI escape code for foreground color
    fn to_ansi_fg(self) -> String {
        match self {
            ColorSpec::Default => String::new(),
            ColorSpec::EightBit(n) => format!("\x1b[38;5;{}m", n),
            ColorSpec::Rgb(r, g, b) => format!("\x1b[38;2;{};{};{}m", r, g, b),
        }
    }

    /// Generate ANSI escape code for background color
    fn to_ansi_bg(self) -> String {
        match self {
            ColorSpec::Default => String::new(),
            ColorSpec::EightBit(n) => format!("\x1b[48;5;{}m", n),
            ColorSpec::Rgb(r, g, b) => format!("\x1b[48;2;{};{};{}m", r, g, b),
        }
    }

    fn is_default(self) -> bool {
        matches!(self, ColorSpec::Default)
    }
}

/// Parse a color value from string
/// Supports:
/// - Named colors: "accent", "dim", "red", etc.
/// - 256-color: "238"
/// - Hex RGB: "#444444" or "#444"
/// - RGB function: "rgb(68,68,68)"
fn parse_color_spec(name: &str) -> ColorSpec {
    let name = name.trim();

    // Check for RGB hex: #RGB or #RRGGBB
    if let Some(hex) = name.strip_prefix('#')
        && let Some((r, g, b)) = parse_hex_color(hex)
    {
        return ColorSpec::Rgb(r, g, b);
    }

    // Check for rgb(r,g,b) syntax
    if let Some(inner) = name.strip_prefix("rgb(").and_then(|s| s.strip_suffix(')'))
        && let Some((r, g, b)) = parse_rgb_func(inner)
    {
        return ColorSpec::Rgb(r, g, b);
    }

    // Check for numeric 256-color
    if let Ok(n) = name.parse::<u8>() {
        return ColorSpec::EightBit(n);
    }

    // Named colors mapped to 256-color approximations
    match name.to_lowercase().as_str() {
        // Default/reset
        "none" | "default" | "reset" => ColorSpec::Default,

        // Theme-like semantic colors (mapped to reasonable 256-color values)
        "accent" | "primary" => ColorSpec::EightBit(39), // Bright blue
        "secondary" => ColorSpec::EightBit(75),          // Light blue
        "tertiary" => ColorSpec::EightBit(141),          // Purple
        "muted" | "quaternary" => ColorSpec::EightBit(245), // Light gray
        "dim" | "dimmed" => ColorSpec::EightBit(240),    // Dark gray

        // Standard colors
        "black" => ColorSpec::EightBit(0),
        "red" | "error" | "warning" => ColorSpec::EightBit(196),
        "green" | "success" | "ok" => ColorSpec::EightBit(82),
        "yellow" => ColorSpec::EightBit(226),
        "blue" => ColorSpec::EightBit(33),
        "magenta" => ColorSpec::EightBit(201),
        "cyan" => ColorSpec::EightBit(51),
        "white" => ColorSpec::EightBit(15),
        "orange" => ColorSpec::EightBit(208),
        "gray" | "grey" => ColorSpec::EightBit(244),
        "pink" => ColorSpec::EightBit(213),
        "purple" => ColorSpec::EightBit(135),

        // Unknown - use default
        _ => ColorSpec::Default,
    }
}

/// Parse hex color: "444444" or "444" -> (r, g, b)
fn parse_hex_color(hex: &str) -> Option<(u8, u8, u8)> {
    match hex.len() {
        3 => {
            // #RGB -> expand to #RRGGBB
            let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
            Some((r, g, b))
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some((r, g, b))
        }
        _ => None,
    }
}

/// Parse "r,g,b" -> (r, g, b)
fn parse_rgb_func(inner: &str) -> Option<(u8, u8, u8)> {
    let parts: Vec<&str> = inner.split(',').collect();
    if parts.len() != 3 {
        return None;
    }
    let r = parts[0].trim().parse::<u8>().ok()?;
    let g = parts[1].trim().parse::<u8>().ok()?;
    let b = parts[2].trim().parse::<u8>().ok()?;
    Some((r, g, b))
}

// ========== STYLE SYSTEM ==========

/// Inline style from #[...] directive
#[derive(Debug, Clone, Default)]
struct InlineStyle {
    fg: ColorSpec,
    bg: ColorSpec,
    bold: bool,
    dim: bool,
    fill: bool,
}

impl InlineStyle {
    /// Generate ANSI escape codes for this style (without reverse - that's handled at line level)
    fn to_ansi(&self) -> String {
        let mut result = String::new();

        // Attributes
        if self.bold {
            result.push_str("\x1b[1m");
        }
        if self.dim {
            result.push_str("\x1b[2m");
        }

        // Colors
        result.push_str(&self.fg.to_ansi_fg());
        result.push_str(&self.bg.to_ansi_bg());

        result
    }

    fn has_any_style(&self) -> bool {
        !self.fg.is_default() || !self.bg.is_default() || self.bold || self.dim || self.fill
    }
}

/// A segment of text with styling
#[derive(Debug, Clone)]
struct StyledSegment {
    text: String,
    style: InlineStyle,
}

impl StyledSegment {
    fn display_width(&self) -> usize {
        self.text.width()
    }
}

/// Collection of styled segments forming a complete styled string
#[derive(Debug, Clone, Default)]
struct StyledText {
    segments: Vec<StyledSegment>,
}

impl StyledText {
    fn new() -> Self {
        Self { segments: vec![] }
    }

    fn push(&mut self, text: String, style: InlineStyle) {
        if !text.is_empty() {
            self.segments.push(StyledSegment { text, style });
        }
    }

    fn display_width(&self) -> usize {
        self.segments.iter().map(|s| s.display_width()).sum()
    }

    /// Render to ANSI-coded string
    fn to_ansi(&self) -> String {
        let mut result = String::new();

        for segment in &self.segments {
            if segment.style.has_any_style() {
                result.push_str("\x1b[0m"); // Reset before applying new style
                result.push_str(&segment.style.to_ansi());
            }
            result.push_str(&segment.text);
        }

        // Reset at end
        if self.segments.iter().any(|s| s.style.has_any_style()) {
            result.push_str("\x1b[0m");
        }

        result
    }

    /// Truncate to fit within max_width display columns
    fn truncate(&self, max_width: usize) -> StyledText {
        if self.display_width() <= max_width {
            return self.clone();
        }

        let mut result = StyledText::new();
        let mut remaining = max_width;

        for segment in &self.segments {
            if remaining == 0 {
                break;
            }

            let seg_width = segment.display_width();
            if seg_width <= remaining {
                result.push(segment.text.clone(), segment.style.clone());
                remaining -= seg_width;
            } else {
                // Truncate this segment
                let mut truncated = String::new();
                let mut width = 0;
                for ch in segment.text.chars() {
                    let ch_width = ch.to_string().width();
                    if width + ch_width > remaining {
                        break;
                    }
                    truncated.push(ch);
                    width += ch_width;
                }
                result.push(truncated, segment.style.clone());
                break;
            }
        }

        result
    }
}

// ========== FORMAT PARSING ==========

/// Token from parsing a tmux-style format string
#[derive(Debug, Clone)]
enum FormatToken {
    /// Style directive: #[fg=color,bg=color,bold,dim]
    Style(InlineStyle),
    /// Variable with optional width: {var}, {=12:var}, or {=<12:var} (left-truncate)
    Variable {
        name: String,
        width: Option<usize>,
        truncate_left: bool,
    },
    /// Plain text
    Literal(String),
}

/// Parse a tmux-style format string into tokens
/// Supports: #[fg=color,bg=color,bold,dim], {variable}, {=width:variable}, #{variable}
fn parse_tmux_format(format: &str) -> Vec<FormatToken> {
    let mut tokens = Vec::new();
    let mut chars = format.chars().peekable();
    let mut literal = String::new();

    while let Some(ch) = chars.next() {
        if ch == '#' {
            match chars.peek() {
                Some('[') => {
                    // Flush literal
                    if !literal.is_empty() {
                        tokens.push(FormatToken::Literal(std::mem::take(&mut literal)));
                    }
                    chars.next(); // consume '['
                    // Parse style directive until ']'
                    let mut style_str = String::new();
                    while let Some(&c) = chars.peek() {
                        if c == ']' {
                            chars.next();
                            break;
                        }
                        style_str.push(chars.next().unwrap());
                    }
                    tokens.push(FormatToken::Style(parse_style_directive(&style_str)));
                }
                Some('{') => {
                    // Flush literal
                    if !literal.is_empty() {
                        tokens.push(FormatToken::Literal(std::mem::take(&mut literal)));
                    }
                    chars.next(); // consume '{'
                    let var_token = parse_variable(&mut chars);
                    tokens.push(var_token);
                }
                _ => {
                    literal.push(ch);
                }
            }
        } else if ch == '{' {
            // Flush literal
            if !literal.is_empty() {
                tokens.push(FormatToken::Literal(std::mem::take(&mut literal)));
            }
            let var_token = parse_variable(&mut chars);
            tokens.push(var_token);
        } else {
            literal.push(ch);
        }
    }

    if !literal.is_empty() {
        tokens.push(FormatToken::Literal(literal));
    }

    tokens
}

/// Parse style directive content: "fg=color,bg=color,bold,dim"
fn parse_style_directive(content: &str) -> InlineStyle {
    let mut style = InlineStyle::default();

    for part in content.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        if let Some(color_str) = part.strip_prefix("fg=") {
            style.fg = parse_color_spec(color_str);
        } else if let Some(color_str) = part.strip_prefix("bg=") {
            style.bg = parse_color_spec(color_str);
        } else if part == "bold" {
            style.bold = true;
        } else if part == "dim" {
            style.dim = true;
        } else if part == "fill" {
            style.fill = true;
        } else if part == "default" || part == "none" || part == "reset" {
            style = InlineStyle::default();
        }
    }

    style
}

/// Parse variable content after '{': "var}" or "=12:var}"
fn parse_variable(chars: &mut std::iter::Peekable<std::str::Chars>) -> FormatToken {
    let mut content = String::new();
    while let Some(&c) = chars.peek() {
        if c == '}' {
            chars.next();
            break;
        }
        content.push(chars.next().unwrap());
    }

    // Check for width specifier: =12:varname (right-truncate)
    // or =<12:varname (left-truncate, keeps end of string, path-aware)
    if let Some(rest) = content.strip_prefix('=')
        && let Some(colon_pos) = rest.find(':')
    {
        let width_spec = &rest[..colon_pos];
        let var_name = &rest[colon_pos + 1..];
        let (truncate_left, width_str) = match width_spec.strip_prefix('<') {
            Some(s) => (true, s),
            None => (false, width_spec),
        };
        if let Ok(width) = width_str.parse::<usize>() {
            return FormatToken::Variable {
                name: var_name.to_string(),
                width: Some(width),
                truncate_left,
            };
        }
    }

    FormatToken::Variable {
        name: content,
        width: None,
        truncate_left: false,
    }
}

/// Parse a styled string like "#[fg=240]│" into StyledText
fn parse_styled_string(s: &str) -> StyledText {
    let tokens = parse_tmux_format(s);
    let mut result = StyledText::new();
    let mut current_style = InlineStyle::default();

    for token in tokens {
        match token {
            FormatToken::Style(style) => {
                current_style = style;
            }
            FormatToken::Literal(text) => {
                result.push(text, current_style.clone());
            }
            FormatToken::Variable { name, .. } => {
                // Variables in border strings are not expanded, treat as literal.
                // (truncate_left flag also ignored here.)
                result.push(format!("{{{}}}", name), current_style.clone());
            }
        }
    }

    result
}

// ========== CONFIGURATION ==========

/// Styling configuration for tab labels
#[derive(Clone)]
struct StyleConfig {
    format: String,
    format_active: String,
    /// Optional second line per tab (e.g. for path/cwd). Empty = single-line mode.
    format_secondary: String,
    format_active_secondary: String,
    overflow_above: String,
    overflow_below: String,
    indicator_active: String,
    indicator_fullscreen: String,
    indicator_sync: String,
    padding_top: usize,
    border: String,
    max_name_length: usize,
    start_index: usize,
}

impl Default for StyleConfig {
    fn default() -> Self {
        Self {
            format: "{index}:{name}".to_string(),
            format_active: "{index}:{name} {indicators}".to_string(),
            format_secondary: String::new(),
            format_active_secondary: String::new(),
            overflow_above: "  ^ +{count}".to_string(),
            overflow_below: "  v +{count}".to_string(),
            indicator_active: "*".to_string(),
            indicator_fullscreen: "Z".to_string(),
            indicator_sync: "S".to_string(),
            max_name_length: 20,
            padding_top: 0,
            border: String::new(),
            start_index: 1,
        }
    }
}

// ========== PLUGIN STATE ==========

#[derive(Default)]
struct State {
    tabs: Vec<TabInfo>,
    active_tab_idx: usize,
    mode_info: ModeInfo,
    pane_manifest: PaneManifest,
    style: StyleConfig,
    last_rows: usize,
    permissions_granted: bool,
    is_selectable: bool,
    pending_events: Vec<Event>,
}

register_plugin!(State);

impl ZellijPlugin for State {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        // Parse style configuration
        if let Some(v) = configuration.get("format") {
            self.style.format = v.clone();
        }
        if let Some(v) = configuration.get("format_active") {
            self.style.format_active = v.clone();
        }
        if let Some(v) = configuration.get("format_secondary") {
            self.style.format_secondary = v.clone();
        }
        if let Some(v) = configuration.get("format_active_secondary") {
            self.style.format_active_secondary = v.clone();
        }
        if let Some(v) = configuration.get("overflow_above") {
            self.style.overflow_above = v.clone();
        }
        if let Some(v) = configuration.get("overflow_below") {
            self.style.overflow_below = v.clone();
        }
        if let Some(v) = configuration.get("indicator_active") {
            self.style.indicator_active = v.clone();
        }
        if let Some(v) = configuration.get("indicator_fullscreen") {
            self.style.indicator_fullscreen = v.clone();
        }
        if let Some(v) = configuration.get("indicator_sync") {
            self.style.indicator_sync = v.clone();
        }
        if let Some(v) = configuration.get("max_name_length")
            && let Ok(n) = v.parse::<usize>()
        {
            self.style.max_name_length = n;
        }
        if let Some(v) = configuration.get("padding_top")
            && let Ok(n) = v.parse::<usize>()
        {
            self.style.padding_top = n;
        }
        if let Some(v) = configuration.get("border") {
            self.style.border = v.clone();
        } else if let Some(v) = configuration.get("border_char") {
            self.style.border = v.clone();
        }
        if let Some(v) = configuration.get("start_index")
            && let Ok(n) = v.parse::<usize>()
        {
            self.style.start_index = n;
        }

        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
        ]);

        subscribe(&[
            EventType::TabUpdate,
            EventType::PaneUpdate,
            EventType::ModeUpdate,
            EventType::Mouse,
            EventType::PermissionRequestResult,
        ]);
    }

    fn update(&mut self, event: Event) -> bool {
        let mut should_render = false;

        if let Event::PermissionRequestResult(status) = event {
            if status == PermissionStatus::Granted {
                self.permissions_granted = true;
                self.is_selectable = false;
                set_selectable(false);

                while !self.pending_events.is_empty() {
                    let cached_event = self.pending_events.remove(0);
                    self.update(cached_event);
                }
                should_render = true;
            }
            return should_render;
        }

        if !self.permissions_granted {
            self.pending_events.push(event);
            return false;
        }

        match event {
            Event::PermissionRequestResult(_) => {}
            Event::ModeUpdate(mode_info) => {
                if self.mode_info != mode_info {
                    should_render = true;
                }
                self.mode_info = mode_info;
            }
            Event::TabUpdate(tabs) => {
                let active_tab_index = tabs.iter().position(|t| t.active).unwrap_or(0);
                let active_tab_idx = active_tab_index + 1;
                if self.active_tab_idx != active_tab_idx || self.tabs != tabs {
                    should_render = true;
                }
                self.active_tab_idx = active_tab_idx;
                self.tabs = tabs;
            }
            Event::PaneUpdate(pane_manifest) => {
                self.pane_manifest = pane_manifest;
                should_render = true;
            }
            Event::Mouse(me) => match me {
                Mouse::LeftClick(row, _col) => {
                    if let Some(idx) = self.get_tab_at_row(row as usize) {
                        switch_tab_to(idx as u32);
                    }
                }
                Mouse::ScrollUp(_) => {
                    let prev_tab = max(self.active_tab_idx.saturating_sub(1), 1);
                    switch_tab_to(prev_tab as u32);
                }
                Mouse::ScrollDown(_) => {
                    let next_tab = min(self.active_tab_idx + 1, self.tabs.len());
                    switch_tab_to(next_tab as u32);
                }
                _ => {}
            },
            _ => {}
        }
        should_render
    }

    fn pipe(&mut self, pipe_message: PipeMessage) -> bool {
        match pipe_message.name.as_str() {
            "set_selectable" => {
                match pipe_message.payload.as_deref() {
                    Some("true") => {
                        self.is_selectable = true;
                        set_selectable(true);
                    }
                    Some("false") => {
                        self.is_selectable = false;
                        set_selectable(false);
                    }
                    _ => {}
                }
                false
            }
            "toggle_selectable" => {
                self.is_selectable = !self.is_selectable;
                set_selectable(self.is_selectable);
                false
            }
            _ => false,
        }
    }

    fn render(&mut self, rows: usize, cols: usize) {
        self.last_rows = rows;

        if !self.permissions_granted || self.tabs.is_empty() {
            return;
        }

        self.render_vertical(rows, cols);
    }
}

impl State {
    /// Returns 2 when a secondary format is configured, else 1.
    fn rows_per_tab(&self) -> usize {
        if self.style.format_secondary.is_empty() && self.style.format_active_secondary.is_empty() {
            1
        } else {
            2
        }
    }

    fn get_focused_pane_title(&self, tab_position: usize) -> Option<String> {
        if let Some(panes) = self.pane_manifest.panes.get(&tab_position) {
            for pane in panes {
                if pane.is_focused && !pane.is_plugin {
                    let title = &pane.title;
                    if title.starts_with("Pane #") || title.starts_with("Tab #") || title.is_empty()
                    {
                        return None;
                    }
                    return Some(title.clone());
                }
            }
        }
        None
    }

    fn expand_overflow_format(&self, format: &str, count: usize) -> String {
        format.replace("{count}", &count.to_string())
    }

    /// Expand a tmux-style format string with tab info, returning styled text
    fn expand_tmux_format(&self, format: &str, tab: &TabInfo, index: usize) -> StyledText {
        let tokens = parse_tmux_format(format);
        let mut result = StyledText::new();
        let mut current_style = InlineStyle::default();

        // Get focused pane title for this tab
        let pane_title = self
            .get_focused_pane_title(tab.position)
            .or_else(|| {
                if !tab.name.starts_with("Tab #") {
                    Some(tab.name.clone())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "...".to_string());

        // Build indicators string
        let mut indicators = String::new();
        if tab.is_fullscreen_active {
            indicators.push_str(&self.style.indicator_fullscreen);
        }
        if tab.is_sync_panes_active {
            indicators.push_str(&self.style.indicator_sync);
        }
        if tab.active {
            indicators.push_str(&self.style.indicator_active);
        }

        for token in tokens {
            match token {
                FormatToken::Style(style) => {
                    current_style = style;
                }
                FormatToken::Variable {
                    name,
                    width,
                    truncate_left,
                } => {
                    let value = match name.as_str() {
                        "index" | "i" => index.to_string(),
                        "name" | "n" => {
                            if tab.active
                                && self.mode_info.mode == InputMode::RenameTab
                                && tab.name.is_empty()
                            {
                                "Enter name...".to_string()
                            } else if !tab.name.starts_with("Tab #") && !tab.name.is_empty() {
                                tab.name.clone()
                            } else {
                                pane_title.clone()
                            }
                        }
                        "title" | "t" | "pane_title" => pane_title.clone(),
                        "indicators" => indicators.clone(),
                        "fullscreen" => {
                            if tab.is_fullscreen_active {
                                self.style.indicator_fullscreen.clone()
                            } else {
                                String::new()
                            }
                        }
                        "sync" => {
                            if tab.is_sync_panes_active {
                                self.style.indicator_sync.clone()
                            } else {
                                String::new()
                            }
                        }
                        "active" => {
                            if tab.active {
                                self.style.indicator_active.clone()
                            } else {
                                String::new()
                            }
                        }
                        _ => format!("{{{}}}", name),
                    };

                    let effective_width = width.unwrap_or(self.style.max_name_length);
                    let text = if truncate_left {
                        truncate_string_left(&value, effective_width)
                    } else {
                        truncate_string(&value, effective_width)
                    };

                    result.push(text, current_style.clone());
                }
                FormatToken::Literal(text) => {
                    result.push(text, current_style.clone());
                }
            }
        }

        result
    }

    /// Build a complete line with content, padding, and border
    fn build_line(&self, content: &StyledText, cols: usize, is_selected: bool) -> String {
        let border = parse_styled_string(&self.style.border);
        let border_width = border.display_width();

        let effective_cols = cols.saturating_sub(border_width);

        // Truncate content if it exceeds available width to prevent wrapping
        let content = content.truncate(effective_cols);
        let content_width = content.display_width();
        let padding_needed = effective_cols.saturating_sub(content_width);

        let mut line = String::new();

        // Check if any segment has fill attribute - fills entire row with bg color
        let has_fill = is_selected && content.segments.iter().any(|s| s.style.fill);

        if has_fill {
            // Fill mode: use reverse video with swapped colors so bg fills the row
            // User writes #[bg=236,fill] -> we swap to fg=236 -> reverse makes displayed bg=236
            line.push_str("\x1b[7m");

            for segment in &content.segments {
                // Swap fg and bg for reverse video
                let mut swapped_style = segment.style.clone();
                std::mem::swap(&mut swapped_style.fg, &mut swapped_style.bg);
                swapped_style.fill = false; // Don't need fill flag in output

                if swapped_style.has_any_style() {
                    line.push_str("\x1b[0m\x1b[7m"); // Reset and re-apply reverse
                    line.push_str(&swapped_style.to_ansi());
                }
                line.push_str(&segment.text);
            }

            if padding_needed > 0 {
                line.push_str(&" ".repeat(padding_needed));
            }

            line.push_str("\x1b[0m");
        } else {
            // Normal rendering - bg colors only apply to text, not padding
            line.push_str(&content.to_ansi());

            if padding_needed > 0 {
                line.push_str(&" ".repeat(padding_needed));
            }
        }

        // Add border (not affected by selection)
        if border_width > 0 {
            line.push_str(&border.to_ansi());
        }

        line
    }

    /// Build a line with just the border (for empty rows)
    fn build_empty_line(&self, cols: usize) -> String {
        let border = parse_styled_string(&self.style.border);
        let border_width = border.display_width();

        if border_width == 0 {
            return " ".repeat(cols);
        }

        let effective_cols = cols.saturating_sub(border_width);
        let mut line = " ".repeat(effective_cols);
        line.push_str(&border.to_ansi());
        line
    }

    fn render_vertical(&mut self, rows: usize, cols: usize) {
        let top_padding = self.style.padding_top;
        let available_rows = rows.saturating_sub(top_padding);

        let tab_count = self.tabs.len();
        let active_index = self.active_tab_idx.saturating_sub(1);
        let rows_per_tab = self.rows_per_tab();

        let (start_index, end_index, tabs_above, tabs_below) =
            calculate_visible_range(tab_count, available_rows, active_index, rows_per_tab);

        let mut lines: Vec<String> = Vec::with_capacity(rows);

        // Add top padding lines
        for _ in 0..top_padding {
            lines.push(self.build_empty_line(cols));
        }

        // Render "above" overflow indicator
        if tabs_above > 0 {
            let indicator_text =
                self.expand_overflow_format(&self.style.overflow_above, tabs_above);
            let styled = parse_styled_string(&indicator_text);
            lines.push(self.build_line(&styled, cols, false));
        }

        // Render visible tabs
        for i in start_index..end_index {
            if let Some(tab) = self.tabs.get(i).cloned() {
                let is_active = tab.active;
                let format = if is_active {
                    &self.style.format_active
                } else {
                    &self.style.format
                };

                let styled = self.expand_tmux_format(format, &tab, i + self.style.start_index);
                lines.push(self.build_line(&styled, cols, is_active));

                // Secondary line (if configured)
                let secondary = if is_active {
                    &self.style.format_active_secondary
                } else {
                    &self.style.format_secondary
                };
                if !secondary.is_empty() {
                    let styled =
                        self.expand_tmux_format(secondary, &tab, i + self.style.start_index);
                    lines.push(self.build_line(&styled, cols, is_active));
                }
            }
        }

        // Render "below" overflow indicator
        if tabs_below > 0 {
            let indicator_text =
                self.expand_overflow_format(&self.style.overflow_below, tabs_below);
            let styled = parse_styled_string(&indicator_text);
            lines.push(self.build_line(&styled, cols, false));
        }

        // Fill remaining rows with empty lines (just border)
        while lines.len() < rows {
            lines.push(self.build_empty_line(cols));
        }

        // Print all lines with ANSI styling
        for (i, line) in lines.iter().enumerate() {
            if i < lines.len() - 1 {
                println!("{}\x1b[m", line);
            } else {
                print!("{}\x1b[m", line);
            }
        }
    }

    fn get_tab_at_row(&self, row: usize) -> Option<usize> {
        if self.tabs.is_empty() {
            return None;
        }

        let tab_count = self.tabs.len();
        let active_index = self.active_tab_idx.saturating_sub(1);
        let rows_per_tab = self.rows_per_tab();

        let (start_index, end_index, tabs_above, _tabs_below) =
            calculate_visible_range(tab_count, self.last_rows, active_index, rows_per_tab);

        let content_start_row = self.style.padding_top + if tabs_above > 0 { 1 } else { 0 };

        if tabs_above > 0 && row == self.style.padding_top {
            let target = start_index.saturating_sub(1);
            return Some(target + 1);
        }

        let row_in_content = row.saturating_sub(content_start_row);
        let clicked_tab_offset = row_in_content / rows_per_tab;
        let clicked_tab_index = start_index + clicked_tab_offset;

        if clicked_tab_index < end_index && clicked_tab_index < tab_count {
            return Some(clicked_tab_index + 1);
        }

        if clicked_tab_offset >= end_index - start_index {
            let target = end_index.min(tab_count.saturating_sub(1));
            return Some(target + 1);
        }

        None
    }
}

fn calculate_visible_range(
    tab_count: usize,
    available_rows: usize,
    active_index: usize,
    rows_per_tab: usize,
) -> (usize, usize, usize, usize) {
    if tab_count == 0 {
        return (0, 0, 0, 0);
    }

    let rows_per_tab = rows_per_tab.max(1);

    if tab_count.saturating_mul(rows_per_tab) <= available_rows {
        return (0, tab_count, 0, 0);
    }

    // 2 rows reserved for overflow indicators (above + below), each 1-row regardless of rows_per_tab.
    let max_visible_rows = available_rows.saturating_sub(2);
    let max_visible = max_visible_rows / rows_per_tab;
    if max_visible == 0 {
        return (0, 0, tab_count, 0);
    }

    let mut start_index = active_index;
    let mut end_index = active_index + 1;
    let mut room_left = max_visible.saturating_sub(1);
    let mut alternate = false;

    while room_left > 0 {
        if !alternate && start_index > 0 {
            start_index -= 1;
            room_left -= 1;
        } else if alternate && end_index < tab_count {
            end_index += 1;
            room_left -= 1;
        } else if start_index > 0 {
            start_index -= 1;
            room_left -= 1;
        } else if end_index < tab_count {
            end_index += 1;
            room_left -= 1;
        } else {
            break;
        }
        alternate = !alternate;
    }

    (
        start_index,
        end_index,
        start_index,
        tab_count.saturating_sub(end_index),
    )
}

fn truncate_string(s: &str, max_width: usize) -> String {
    if s.width() <= max_width {
        return s.to_string();
    }

    if max_width <= 3 {
        return ".".repeat(max_width);
    }

    let mut truncated = String::new();
    let mut width = 0;
    for ch in s.chars() {
        let ch_width = ch.to_string().width();
        if width + ch_width + 3 > max_width {
            truncated.push_str("...");
            break;
        }
        truncated.push(ch);
        width += ch_width;
    }
    truncated
}

/// Truncate from the LEFT, keeping the end of the string. Adds "…" prefix.
/// Path-aware: if the string contains '/', snaps the cut to the longest
/// path-component suffix that fits within max_width.
fn truncate_string_left(s: &str, max_width: usize) -> String {
    if s.width() <= max_width {
        return s.to_string();
    }
    if max_width <= 1 {
        return "…".to_string();
    }

    let ellipsis = "…";
    let ellipsis_width = ellipsis.width();

    // Path-aware: try each '/' position, picking the longest suffix that fits.
    let slash_positions: Vec<usize> = s
        .char_indices()
        .filter(|(_, c)| *c == '/')
        .map(|(i, _)| i)
        .collect();

    for &pos in &slash_positions {
        let suffix = &s[pos..];
        if suffix.width() + ellipsis_width <= max_width {
            return format!("{}{}", ellipsis, suffix);
        }
    }

    // No path-boundary fit (or no slashes): char-level left-truncate.
    let target_width = max_width.saturating_sub(ellipsis_width);
    let mut suffix_chars: Vec<char> = Vec::new();
    let mut width = 0;
    for ch in s.chars().rev() {
        let ch_width = ch.to_string().width();
        if width + ch_width > target_width {
            break;
        }
        suffix_chars.push(ch);
        width += ch_width;
    }
    suffix_chars.reverse();
    let suffix: String = suffix_chars.iter().collect();
    format!("{}{}", ellipsis, suffix)
}
