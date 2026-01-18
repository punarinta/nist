use crate::screen_buffer::ScreenBuffer;
use std::io::Write;
use std::sync::{Arc, Mutex};
use unicode_segmentation::UnicodeSegmentation;

pub(crate) fn process_output(
    text: &str,
    screen_buffer: &Arc<Mutex<ScreenBuffer>>,
    saved_screen_buffer: &Arc<Mutex<Vec<ScreenBuffer>>>,
    writer: &Arc<Mutex<Box<dyn std::io::Write + Send>>>,
    last_command_exit_code: &Arc<Mutex<Option<i32>>>,
    default_cursor_style: &Arc<Mutex<crate::screen_buffer::CursorStyle>>,
) -> String {
    let mut incomplete_sequence = String::new();

    let mut sb = screen_buffer.lock().unwrap();
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '\x1b' => {
                // Start of escape sequence
                let mut sequence = String::new();
                sequence.push(ch);

                // Look ahead to see what type of sequence this is
                if let Some(&next_ch) = chars.peek() {
                    match next_ch {
                        '[' => {
                            // CSI sequence
                            sequence.push(chars.next().unwrap()); // consume '['

                            let mut found_end = false;
                            while let Some(&_peek_ch) = chars.peek() {
                                let ch = chars.next().unwrap();
                                sequence.push(ch);

                                // CSI sequences end with a letter in range 0x40-0x7E
                                if ('@'..='~').contains(&ch) {
                                    found_end = true;
                                    break;
                                }
                            }

                            if !found_end {
                                // Incomplete sequence, save it for next iteration
                                incomplete_sequence = sequence;
                                break;
                            }

                            // Process complete CSI sequence
                            process_csi_sequence(&sequence, &mut sb, saved_screen_buffer, writer);
                        }
                        ']' => {
                            // OSC (Operating System Command) sequence
                            // Format: ESC ] <number> ; <text> BEL (or ESC \)
                            // Example: ESC ] 0 ; title BEL (set window title)
                            // Example: ESC ] 1337 ; command-exit=<code> BEL (command exit code)
                            sequence.push(chars.next().unwrap()); // consume ']'

                            let mut found_end = false;
                            while let Some(&_peek_ch) = chars.peek() {
                                let ch = chars.next().unwrap();
                                sequence.push(ch);

                                // OSC sequences end with BEL (0x07) or ST (ESC \)
                                if ch == '\x07' {
                                    found_end = true;
                                    break;
                                }
                                // Check for ST (ESC \) - need to check if previous char was ESC
                                if ch == '\\' && sequence.len() >= 2 && sequence.chars().nth(sequence.len() - 2) == Some('\x1b') {
                                    found_end = true;
                                    break;
                                }
                            }

                            if !found_end {
                                // Incomplete sequence, save it for next iteration
                                incomplete_sequence = sequence;
                                break;
                            }

                            // Parse OSC sequences for command exit codes
                            // Format: ESC ] 1337 ; command-exit=<code> BEL
                            if sequence.contains("1337;command-exit=") {
                                if let Some(exit_code_str) = sequence
                                    .split("command-exit=")
                                    .nth(1)
                                    .and_then(|s| s.split('\x07').next())
                                    .and_then(|s| s.split('\\').next())
                                {
                                    if let Ok(exit_code) = exit_code_str.trim().parse::<i32>() {
                                        eprintln!("[TERMINAL] Command exited with code: {}", exit_code);
                                        if let Ok(mut last_exit) = last_command_exit_code.lock() {
                                            *last_exit = Some(exit_code);
                                        }
                                        // Reset cursor to default style when command exits
                                        if let Ok(default_style) = default_cursor_style.lock() {
                                            sb.cursor_style = *default_style;
                                            eprintln!("[TERMINAL] Reset cursor to default style: {:?}", *default_style);
                                        }
                                    }
                                }
                            }

                            // OSC sequences are for terminal control (titles, etc.), not for display
                            // They should not be rendered
                        }
                        '(' | ')' | '*' | '+' => {
                            // Character set designation sequences
                            // ESC ( C - Designate G0 Character Set
                            // ESC ) C - Designate G1 Character Set
                            // ESC * C - Designate G2 Character Set
                            // ESC + C - Designate G3 Character Set
                            let designator = chars.next().unwrap(); // consume the designation char
                            sequence.push(designator);
                            if let Some(charset_ch) = chars.next() {
                                sequence.push(charset_ch);

                                // Determine which G set to designate
                                let g_set = match designator {
                                    '(' => 0, // G0
                                    ')' => 1, // G1
                                    '*' => 2, // G2
                                    '+' => 3, // G3
                                    _ => 0,
                                };

                                // Determine which charset
                                let charset = match charset_ch {
                                    'B' => crate::screen_buffer::CharSet::Ascii,
                                    '0' => crate::screen_buffer::CharSet::DecSpecialGraphics,
                                    // Other charsets like 'A' (UK), '1', '2' etc. default to ASCII
                                    _ => crate::screen_buffer::CharSet::Ascii,
                                };

                                sb.designate_charset(g_set, charset);
                            } else {
                                incomplete_sequence = sequence;
                                break;
                            }
                        }
                        'n' => {
                            // LS2 - Locking Shift 2 - Invoke G2 Character Set as GL
                            chars.next(); // consume 'n'
                            sb.locking_shift(2);
                        }
                        'o' => {
                            // LS3 - Locking Shift 3 - Invoke G3 Character Set as GL
                            chars.next(); // consume 'o'
                            sb.locking_shift(3);
                        }
                        '|' => {
                            // LS3R - Locking Shift 3 Right - Invoke G3 Character Set as GR
                            chars.next(); // consume '|'
                            sb.locking_shift(3);
                        }
                        '}' => {
                            // LS2R - Locking Shift 2 Right - Invoke G2 Character Set as GR
                            chars.next(); // consume '}'
                            sb.locking_shift(2);
                        }
                        '~' => {
                            // LS1R - Locking Shift 1 Right - Invoke G1 Character Set as GR
                            chars.next(); // consume '~'
                            sb.locking_shift(1);
                        }
                        'N' => {
                            // SS2 - Single Shift 2 - Invoke G2 Character Set for next character only
                            chars.next(); // consume 'N'
                            sb.single_shift(2);
                        }
                        'O' => {
                            // SS3 - Single Shift 3 - Invoke G3 Character Set for next character only
                            chars.next(); // consume 'O'
                            sb.single_shift(3);
                        }
                        'Z' => {
                            // DECID - Return Terminal ID (obsolete form of CSI c)
                            // Respond with CSI ? 6 c (VT102)
                            chars.next(); // consume 'Z'
                            let response = "\x1b[?6c";
                            if let Ok(mut w) = writer.lock() {
                                let _ = w.write_all(response.as_bytes());
                                let _ = w.flush();
                            }
                        }
                        'c' => {
                            // RIS (Reset to Initial State)
                            chars.next(); // consume 'c'
                            sb.clear_screen();
                        }
                        '6' => {
                            // DECBI (Back Index) - VT Level 4
                            // Move cursor backward one column
                            // If at left margin, scroll content right (not commonly implemented)
                            chars.next(); // consume '6'
                            if sb.cursor_x > 0 {
                                sb.move_cursor_left(1);
                            }
                            // Full implementation would scroll content right when at left margin
                            // but this is rarely used, so we just stop at the left edge
                        }
                        '7' => {
                            // Save cursor position (DECSC)
                            chars.next(); // consume '7'
                            sb.save_cursor();
                        }
                        '8' => {
                            // Restore cursor position (DECRC)
                            chars.next(); // consume '8'
                            sb.restore_cursor();
                        }
                        '9' => {
                            // DECFI (Forward Index) - VT Level 4
                            // Move cursor forward one column
                            // If at right margin, scroll content left (not commonly implemented)
                            chars.next(); // consume '9'
                            if sb.cursor_x < sb.width() - 1 {
                                sb.move_cursor_right(1);
                            }
                            // Full implementation would scroll content left when at right margin
                            // but this is rarely used, so we just stop at the right edge
                        }
                        'D' => {
                            // IND (Index) - move cursor down one line
                            // If at bottom of scroll region, scroll up instead
                            chars.next(); // consume 'D'

                            let scroll_bottom = if let Some((_, bottom)) = sb.get_scroll_region() {
                                bottom
                            } else {
                                sb.height() - 1
                            };

                            if sb.cursor_y == scroll_bottom {
                                // At bottom of scroll region, scroll up
                                sb.scroll_up(1);
                            } else {
                                sb.move_cursor_down(1);
                            }
                        }
                        'M' => {
                            // RI (Reverse Index) - move cursor up one line
                            // If at top of scroll region, scroll down instead
                            chars.next(); // consume 'M'

                            let scroll_top = if let Some((top, _)) = sb.get_scroll_region() { top } else { 0 };

                            if sb.cursor_y == scroll_top {
                                // At top of scroll region, scroll down
                                sb.scroll_down(1);
                            } else {
                                sb.move_cursor_up(1);
                            }
                        }
                        'E' => {
                            // NEL (Next Line)
                            chars.next(); // consume 'E'
                            sb.cursor_x = 0;
                            sb.move_cursor_down(1);
                        }
                        'H' => {
                            // HTS (Horizontal Tab Set) - Set tab stop at current column
                            chars.next(); // consume 'H'
                            sb.set_tab_stop();
                        }
                        '=' => {
                            // DECKPAM (Keypad Application Mode)
                            chars.next(); // consume '='
                                          // We can ignore this for now
                        }
                        '>' => {
                            // DECKPNM (Keypad Numeric Mode)
                            chars.next(); // consume '>'
                                          // We can ignore this for now
                        }
                        _ => {
                            // Unknown escape sequence, just consume the next character
                            chars.next();
                        }
                    }
                } else {
                    // Incomplete escape sequence
                    incomplete_sequence = sequence;
                    break;
                }
            }
            '\r' => {
                // Carriage return
                sb.pending_wrap = false;
                sb.cursor_x = 0;
                // If automatic newline mode is enabled, CR acts as CR+LF
                if sb.get_automatic_newline() {
                    sb.newline();
                }
            }
            '\n' => {
                // Line feed
                sb.newline();
            }
            '\x0b' => {
                // VT (Vertical Tab, Ctrl-K) - behave like line feed
                sb.newline();
            }
            '\t' => {
                // Tab
                sb.tab();
            }
            '\x08' => {
                // Backspace
                sb.move_cursor_left(1);
            }
            '\x0c' => {
                // Form feed - clear screen
                sb.clear_screen();
            }
            '\x05' => {
                // ENQ (Enquiry, Ctrl-E) - Return Terminal Status
                // Default response is empty string, or answerback string resource
                // We send an empty response to acknowledge the enquiry
                if let Ok(mut w) = writer.lock() {
                    let _ = w.write_all(b"");
                    let _ = w.flush();
                }
            }
            '\x07' => {
                // Bell - we can ignore this or implement a visual bell
            }
            '\x0e' => {
                // SO (Shift Out, Ctrl-N) - Switch to G1 character set
                sb.shift_out();
            }
            '\x0f' => {
                // SI (Shift In, Ctrl-O) - Switch to G0 character set
                sb.shift_in();
            }
            ch if ch.is_control() => {
                // Other control characters - ignore for now
            }
            _ => {
                // Regular character - collect into a buffer to handle grapheme clusters
                // We need to collect all text until the next control character
                let mut text_buffer = String::new();
                text_buffer.push(ch);

                // Peek ahead and collect more characters until we hit a control char
                while let Some(&peek_ch) = chars.peek() {
                    if peek_ch.is_control() || peek_ch == '\x1b' {
                        break;
                    }
                    text_buffer.push(chars.next().unwrap());
                }

                // Now process the buffer as grapheme clusters
                for grapheme in text_buffer.graphemes(true) {
                    sb.put_grapheme(grapheme);
                }
            }
        }
    }

    incomplete_sequence
}

pub(crate) fn process_csi_sequence(
    sequence: &str,
    sb: &mut ScreenBuffer,
    saved_screen_buffer: &Arc<Mutex<Vec<ScreenBuffer>>>,
    writer: &Arc<Mutex<Box<dyn std::io::Write + Send>>>,
) {
    use crate::ansi;

    let debug = false; // Set to true for debugging

    if debug {
        eprintln!("[TERMINAL] Processing CSI: {:?}", sequence);
    }

    // Handle DECSTR (Soft Terminal Reset) - CSI ! p
    if sequence.ends_with("!p") {
        eprintln!("[TERMINAL] Performing soft terminal reset (DECSTR)");
        sb.soft_reset();
        return;
    }

    // Handle DECRQM (Request Mode) - CSI ? Ps $ p
    // Applications like opencode query mode status and wait for responses
    // We respond conservatively to unblock the application
    if sequence.contains("$p") {
        let mode_query = sequence.trim_start_matches("\x1b[");
        if mode_query.starts_with('?') && mode_query.ends_with("$p") {
            let mode_str = mode_query.trim_start_matches('?').trim_end_matches("$p");
            if let Ok(mode_num) = mode_str.parse::<u32>() {
                // Status: 0=not recognized, 1=set, 2=reset, 3=permanently set, 4=permanently reset
                // We conservatively report modes as either not recognized (0) or reset (2)
                let status = match mode_num {
                    1 | 1000 | 1002 | 1003 | 1004 | 1006 | 1016 | 2004 | 2026 | 2027 | 2031 => {
                        // Known modes - report as reset (off)
                        2
                    }
                    _ => {
                        // Unknown mode - report as not recognized
                        0
                    }
                };

                // Send DECRPM response: CSI ? Ps ; Pm $ y
                let response = format!("\x1b[?{};{}$y", mode_num, status);
                if let Ok(mut w) = writer.lock() {
                    if let Err(e) = w.write_all(response.as_bytes()) {
                        eprintln!("[DECRQM] Failed to send mode report for mode {}: {}", mode_num, e);
                    } else if let Err(e) = w.flush() {
                        eprintln!("[DECRQM] Failed to flush mode report: {}", e);
                    }
                }
            }
            return;
        }
    }

    // Extract the final character and arguments
    let chars: Vec<char> = sequence.chars().collect();
    if chars.len() < 3 {
        return;
    }

    let final_char = chars[chars.len() - 1];
    let args_str = sequence.trim_start_matches("\x1b[").trim_end_matches(final_char);
    let args: Vec<&str> = if args_str.is_empty() { vec![] } else { args_str.split(';').collect() };

    match final_char {
        'A' => {
            let n = if args.is_empty() || args[0].is_empty() {
                1
            } else {
                args[0].parse::<usize>().unwrap_or(1)
            };
            sb.move_cursor_up(n);
        }
        'B' => {
            let n = if args.is_empty() || args[0].is_empty() {
                1
            } else {
                args[0].parse::<usize>().unwrap_or(1)
            };
            sb.move_cursor_down(n);
        }
        'C' => {
            let n = if args.is_empty() || args[0].is_empty() {
                1
            } else {
                args[0].parse::<usize>().unwrap_or(1)
            };
            sb.move_cursor_right(n);
        }
        'D' => {
            let n = if args.is_empty() || args[0].is_empty() {
                1
            } else {
                args[0].parse::<usize>().unwrap_or(1)
            };
            sb.move_cursor_left(n);
        }
        'E' => {
            // CNL (Cursor Next Line)
            let n = if args.is_empty() || args[0].is_empty() {
                1
            } else {
                args[0].parse::<usize>().unwrap_or(1)
            };
            sb.move_cursor_down(n);
            sb.cursor_x = 0;
            sb.pending_wrap = false;
        }
        'F' => {
            // CPL (Cursor Previous Line)
            let n = if args.is_empty() || args[0].is_empty() {
                1
            } else {
                args[0].parse::<usize>().unwrap_or(1)
            };
            sb.move_cursor_up(n);
            sb.cursor_x = 0;
            sb.pending_wrap = false;
        }
        'G' => {
            // CHA (Cursor Horizontal Absolute)
            let col = if args.is_empty() || args[0].is_empty() {
                1
            } else {
                args[0].parse::<usize>().unwrap_or(1)
            };
            sb.pending_wrap = false;
            sb.cursor_x = col.saturating_sub(1).min(sb.width() - 1);
        }
        '`' => {
            // HPA (Horizontal Position Absolute) - same as CHA but uses backtick
            let col = if args.is_empty() || args[0].is_empty() {
                1
            } else {
                args[0].parse::<usize>().unwrap_or(1)
            };
            sb.pending_wrap = false;
            sb.cursor_x = col.saturating_sub(1).min(sb.width() - 1);
        }
        'H' | 'f' => {
            // CUP (Cursor Position) and HVP (Horizontal and Vertical Position)
            // Both work identically: CSI row ; col H/f
            if args.is_empty() || (args.len() == 1 && args[0].is_empty()) {
                sb.move_cursor_to(0, 0);
            } else {
                let [row, col] = ansi::parse_capital_h(sequence);
                let target_row = (row as usize).saturating_sub(1);
                let target_col = (col as usize).saturating_sub(1);
                sb.move_cursor_to(target_col, target_row);
            }
        }
        'I' => {
            // CHT (Cursor Horizontal Forward Tabulation)
            let n = if args.is_empty() || args[0].is_empty() {
                1
            } else {
                args[0].parse::<usize>().unwrap_or(1)
            };
            sb.forward_tab(n);
        }
        'J' => {
            // Check if this is DECSED (Selective Erase in Display) with '?' prefix
            if args_str.starts_with('?') {
                // DECSED - Selective Erase in Display
                // Erase only unprotected characters (we don't support protection attributes)
                // For simplicity, treat this the same as regular ED
                let arg_str = args_str.trim_start_matches('?');
                let arg = if arg_str.is_empty() { 0 } else { arg_str.parse::<usize>().unwrap_or(0) };
                match arg {
                    0 => sb.clear_from_cursor_to_end(),
                    1 => sb.clear_from_start_to_cursor(),
                    2 | 3 => sb.clear_screen(),
                    _ => {}
                }
            } else {
                // ED (Erase in Display)
                let arg = if args.is_empty() || args[0].is_empty() {
                    0
                } else {
                    args[0].parse::<usize>().unwrap_or(0)
                };
                match arg {
                    0 => {
                        sb.clear_from_cursor_to_end();
                    }
                    1 => {
                        sb.clear_from_start_to_cursor();
                    }
                    2 | 3 => {
                        sb.clear_screen();
                    }
                    _ => {}
                }
            }
        }
        'K' => {
            // Check if this is DECSEL (Selective Erase in Line) with '?' prefix
            if args_str.starts_with('?') {
                // DECSEL - Selective Erase in Line
                // Erase only unprotected characters (we don't support protection attributes)
                // For simplicity, treat this the same as regular EL
                let arg_str = args_str.trim_start_matches('?');
                let arg = if arg_str.is_empty() { 0 } else { arg_str.parse::<usize>().unwrap_or(0) };
                match arg {
                    0 => sb.clear_line_from_cursor(),
                    1 => sb.clear_line_to_cursor(),
                    2 => sb.clear_line(),
                    _ => {}
                }
            } else {
                // EL (Erase in Line)
                let arg = if args.is_empty() || args[0].is_empty() {
                    0
                } else {
                    args[0].parse::<usize>().unwrap_or(0)
                };
                match arg {
                    0 => {
                        sb.clear_line_from_cursor();
                    }
                    1 => {
                        sb.clear_line_to_cursor();
                    }
                    2 => {
                        sb.clear_line();
                    }
                    _ => {}
                }
            }
        }
        'r' => {
            // DECSTBM - Set scrolling region (top;bottom)
            // CSI Ps ; Ps r - Sets the top and bottom margins for scrolling
            // This is used by applications like vim, less, mc, etc.
            // Per VT100 spec, this command also moves cursor to home position
            if args.is_empty() || (args.len() == 1 && args[0].is_empty()) {
                // No arguments means reset to full screen
                sb.reset_scroll_region();
                sb.move_cursor_to(0, 0);
            } else {
                let [top, bottom] = ansi::parse_scroll_region(sequence);
                if debug {
                    eprintln!("[TERMINAL] Setting scroll region: top={}, bottom={}", top, bottom);
                }
                // Convert from 1-based ANSI coordinates to 0-based indices
                let top_idx = (top - 1).max(0) as usize;
                let bottom_idx = if bottom == -1 {
                    sb.height().saturating_sub(1)
                } else {
                    (bottom - 1).max(0) as usize
                };
                sb.set_scroll_region(top_idx, bottom_idx);
                sb.move_cursor_to(0, 0);
            }
        }
        'h' | 'l' => {
            // Set Mode (h) or Reset Mode (l)
            let mode_numbers = parse_mode_sequences_old(&sequence[2..sequence.len() - 1], debug);

            for mode_str in mode_numbers {
                if debug {
                    eprintln!("[TERMINAL] Processing mode: {} ({})", mode_str, if final_char == 'h' { "set" } else { "reset" });
                }

                match mode_str.as_str() {
                    "47" | "?47" => {
                        // Alternate screen buffer (basic version without cursor save)
                        // CSI ? 47 h - Use Alternate Screen Buffer
                        // CSI ? 47 l - Use Normal Screen Buffer
                        if final_char == 'h' {
                            eprintln!("[ALTSCREEN] Switching TO alternate screen buffer (mode 47)");
                            let mut saved_stack = saved_screen_buffer.lock().unwrap();
                            saved_stack.push(sb.clone());
                            let scrollback_limit = sb.scrollback_limit();
                            *sb = ScreenBuffer::new_with_scrollback(sb.width(), sb.height(), scrollback_limit, sb.cursor_style);
                        } else {
                            eprintln!("[ALTSCREEN] Switching FROM alternate screen buffer (mode 47)");
                            let mut saved_stack = saved_screen_buffer.lock().unwrap();
                            if let Some(mut saved_sb) = saved_stack.pop() {
                                if saved_sb.width() != sb.width() || saved_sb.height() != sb.height() {
                                    saved_sb.resize(sb.width(), sb.height());
                                }
                                *sb = saved_sb;
                            }
                        }
                    }
                    "1047" | "?1047" => {
                        // Alternate screen buffer with clearing
                        // CSI ? 1047 h - Use Alternate Screen Buffer, clearing it first if in alternate
                        // CSI ? 1047 l - Use Normal Screen Buffer
                        if final_char == 'h' {
                            eprintln!("[ALTSCREEN] Switching TO alternate screen buffer (mode 1047)");
                            let mut saved_stack = saved_screen_buffer.lock().unwrap();
                            saved_stack.push(sb.clone());
                            let scrollback_limit = sb.scrollback_limit();
                            *sb = ScreenBuffer::new_with_scrollback(sb.width(), sb.height(), scrollback_limit, sb.cursor_style);
                        } else {
                            eprintln!("[ALTSCREEN] Switching FROM alternate screen buffer (mode 1047)");
                            sb.clear_screen();
                            let mut saved_stack = saved_screen_buffer.lock().unwrap();
                            if let Some(mut saved_sb) = saved_stack.pop() {
                                if saved_sb.width() != sb.width() || saved_sb.height() != sb.height() {
                                    saved_sb.resize(sb.width(), sb.height());
                                }
                                *sb = saved_sb;
                            }
                        }
                    }
                    "1048" | "?1048" => {
                        // Save/Restore cursor position (DECSC/DECRC style)
                        // CSI ? 1048 h - Save cursor position
                        // CSI ? 1048 l - Restore cursor position
                        if final_char == 'h' {
                            sb.save_cursor();
                        } else {
                            sb.restore_cursor();
                        }
                    }
                    "1049" | "?1049" => {
                        // Alternate screen buffer (supports stacking for nested alternate screens)
                        // Per xterm spec (XFree86 ctlseqs):
                        // CSI ? 1049 h - Save cursor as in DECSC and use Alternate Screen Buffer, clearing it first
                        // CSI ? 1049 l - Use Normal Screen Buffer and restore cursor as in DECRC
                        if final_char == 'h' {
                            eprintln!("[ALTSCREEN] Switching TO alternate screen buffer (save cursor + switch)");
                            // Save cursor position (implicit DECSC per xterm spec)
                            sb.save_cursor();

                            // Save current screen to stack and switch to alternate
                            let mut saved_stack = saved_screen_buffer.lock().unwrap();
                            // Save the current (main) buffer
                            saved_stack.push(sb.clone());

                            // Create a BRAND NEW empty buffer for alternate screen
                            // This prevents any content from the main screen bleeding through
                            let scrollback_limit = sb.scrollback_limit();
                            *sb = ScreenBuffer::new_with_scrollback(sb.width(), sb.height(), scrollback_limit, sb.cursor_style);
                        } else {
                            eprintln!("[ALTSCREEN] Switching FROM alternate screen buffer (restore main + cursor)");
                            // Per xterm spec, clear the alternate screen before switching back
                            sb.clear_screen();

                            // Restore screen from stack
                            let mut saved_stack = saved_screen_buffer.lock().unwrap();
                            if let Some(mut saved_sb) = saved_stack.pop() {
                                // Check if dimensions match, resize saved buffer if needed
                                if saved_sb.width() != sb.width() || saved_sb.height() != sb.height() {
                                    saved_sb.resize(sb.width(), sb.height());
                                }
                                *sb = saved_sb;
                                // Restore cursor position (implicit DECRC per xterm spec)
                                // The saved cursor was stored in the saved_sb before we switched to altscreen
                                sb.restore_cursor();
                            }
                        }
                    }
                    // Note: Mode 25 (cursor visibility) and Mode 1 (application cursor keys)
                    // are already handled by parse_mode_sequences() function
                    "6" | "?6" => {
                        // DECOM - Origin mode
                        // When enabled, cursor positioning is relative to scroll region
                        if final_char == 'h' {
                            sb.set_origin_mode(true);
                            sb.move_cursor_to(0, 0); // Move to home position (top-left of scroll region)
                        } else {
                            sb.set_origin_mode(false);
                            sb.move_cursor_to(0, 0); // Move to home position (top-left of screen)
                        }
                    }
                    "5" | "?5" => {
                        // DECSCNM - Reverse Video Mode
                        // When enabled, swap all foreground/background colors globally
                        if final_char == 'h' {
                            eprintln!("[TERMINAL] Enabling reverse video mode");
                            sb.reverse_video_mode = true;
                        } else {
                            eprintln!("[TERMINAL] Disabling reverse video mode");
                            sb.reverse_video_mode = false;
                        }
                        sb.dirty = true;
                    }
                    "7" | "?7" => {
                        // DECAWM - Auto-wrap mode
                        // When enabled, cursor wraps to next line at right margin
                        if final_char == 'h' {
                            sb.set_auto_wrap_mode(true);
                        } else {
                            sb.set_auto_wrap_mode(false);
                            // Clear any pending wrap when disabling auto-wrap
                            sb.pending_wrap = false;
                        }
                    }
                    "4" => {
                        // IRM - Insert Mode (standard mode, not DEC private)
                        // When enabled, inserting characters pushes existing ones to the right
                        if final_char == 'h' {
                            sb.set_insert_mode(true);
                        } else {
                            sb.set_insert_mode(false);
                        }
                    }
                    "20" => {
                        // LNM - Automatic Newline Mode (standard mode, not DEC private)
                        // When enabled, CR (Ctrl-M) acts as CR+LF
                        if final_char == 'h' {
                            sb.set_automatic_newline(true);
                        } else {
                            sb.set_automatic_newline(false);
                        }
                    }

                    "?1000" | "?1002" | "?1003" => {
                        // Mouse reporting modes - we can ignore for now
                    }
                    "?1006" => {
                        // SGR mouse mode - we can ignore for now
                    }
                    "?2026" => {
                        // Synchronized output mode (BSU - Begin Synchronized Update)
                        // This is used by TUI apps to batch screen updates and prevent flickering
                        // We don't need to do anything special here - just acknowledge the mode
                    }
                    "?2027" => {
                        // End synchronized output mode (ESU)
                        // We don't need to do anything special here
                    }
                    "?2004" => {
                        // Bracketed paste mode - we can ignore for now
                    }
                    "?4" => {
                        // DECSCLM - Smooth Scroll Mode
                        // When enabled, scrolling is smooth (animated)
                        // We don't animate scrolling, so this is a no-op
                        // Just acknowledge the mode without error
                        if final_char == 'h' {
                            if debug {
                                eprintln!("[TERMINAL] Smooth scroll mode enabled (no-op)");
                            }
                        } else {
                            if debug {
                                eprintln!("[TERMINAL] Smooth scroll mode disabled (no-op)");
                            }
                        }
                    }
                    "?12" => {
                        // Start/Stop Blinking Cursor (att610)
                        // When enabled, cursor blinks; when disabled, cursor is steady
                        use crate::screen_buffer::CursorStyle;
                        if final_char == 'h' {
                            // Enable blinking - convert current cursor style to blinking variant
                            sb.cursor_style = match sb.cursor_style {
                                CursorStyle::SteadyBlock => CursorStyle::BlinkingBlock,
                                CursorStyle::SteadyUnderline => CursorStyle::BlinkingUnderline,
                                CursorStyle::SteadyBar => CursorStyle::BlinkingBar,
                                _ => sb.cursor_style, // Already blinking or default
                            };
                            if debug {
                                eprintln!("[TERMINAL] Cursor blinking enabled");
                            }
                        } else {
                            // Disable blinking - convert current cursor style to steady variant
                            sb.cursor_style = match sb.cursor_style {
                                CursorStyle::BlinkingBlock => CursorStyle::SteadyBlock,
                                CursorStyle::BlinkingUnderline => CursorStyle::SteadyUnderline,
                                CursorStyle::BlinkingBar => CursorStyle::SteadyBar,
                                _ => sb.cursor_style, // Already steady
                            };
                            if debug {
                                eprintln!("[TERMINAL] Cursor blinking disabled");
                            }
                        }
                    }
                    "?40" => {
                        // Allow 80→132 Column Mode
                        // This would allow switching between 80 and 132 column modes
                        // We don't implement dynamic column switching, so this is a no-op
                        if debug {
                            if final_char == 'h' {
                                eprintln!("[TERMINAL] Allow 80→132 mode enabled (no-op)");
                            } else {
                                eprintln!("[TERMINAL] Allow 80→132 mode disabled (no-op)");
                            }
                        }
                    }
                    "?45" => {
                        // Reverse-wraparound Mode
                        // When enabled, cursor can wrap backwards from left margin to previous line
                        // This is a complex feature that's rarely used
                        // For now, treat as no-op
                        if debug {
                            if final_char == 'h' {
                                eprintln!("[TERMINAL] Reverse-wraparound mode enabled (no-op)");
                            } else {
                                eprintln!("[TERMINAL] Reverse-wraparound mode disabled (no-op)");
                            }
                        }
                    }
                    "?66" => {
                        // DECNKM - Application Keypad Mode
                        // Controls whether numeric keypad sends application sequences
                        // This affects input handling, not output, so we acknowledge but don't act
                        if debug {
                            if final_char == 'h' {
                                eprintln!("[TERMINAL] Application keypad mode enabled (no-op)");
                            } else {
                                eprintln!("[TERMINAL] Application keypad mode disabled (no-op)");
                            }
                        }
                    }
                    "?67" => {
                        // DECBKM - Backarrow Key Mode
                        // Controls whether backspace key sends BS or DEL
                        // This affects input handling, not output, so we acknowledge but don't act
                        if debug {
                            if final_char == 'h' {
                                eprintln!("[TERMINAL] Backarrow key sends backspace (no-op)");
                            } else {
                                eprintln!("[TERMINAL] Backarrow key sends delete (no-op)");
                            }
                        }
                    }
                    "?8" => {
                        // DECARM - Auto-repeat Keys
                        // Terminal emulator can't control keyboard auto-repeat
                        // This is handled by the OS, so we acknowledge but don't act
                        if debug {
                            if final_char == 'h' {
                                eprintln!("[TERMINAL] Auto-repeat keys enabled (no-op)");
                            } else {
                                eprintln!("[TERMINAL] Auto-repeat keys disabled (no-op)");
                            }
                        }
                    }
                    _ => {
                        if debug {
                            eprintln!("[TERMINAL] Ignoring unknown mode: {}", mode_str);
                        }
                    }
                }
            }
        }
        'L' => {
            // Insert Line(s) - CSI Ps L
            // Insert Ps blank lines at cursor position
            let n = if args.is_empty() || args[0].is_empty() {
                1
            } else {
                args[0].parse::<usize>().unwrap_or(1)
            };
            sb.insert_lines(n);
        }
        'M' => {
            // Delete Line(s) - CSI Ps M
            // Delete Ps lines starting at cursor position
            let n = if args.is_empty() || args[0].is_empty() {
                1
            } else {
                args[0].parse::<usize>().unwrap_or(1)
            };
            sb.delete_lines(n);
        }
        'S' => {
            // Scroll Up - CSI Ps S
            // Scroll up Ps lines (default = 1)
            // This scrolls the content within the scrolling region
            let n = if args.is_empty() || args[0].is_empty() {
                1
            } else {
                args[0].parse::<usize>().unwrap_or(1)
            };
            sb.scroll_up(n);
        }
        'T' => {
            // Scroll Down - CSI Ps T
            // Scroll down Ps lines (default = 1)
            let n = if args.is_empty() || args[0].is_empty() {
                1
            } else {
                args[0].parse::<usize>().unwrap_or(1)
            };
            sb.scroll_down(n);
        }
        '@' => {
            // Insert Characters - CSI Ps @
            let n = if args.is_empty() || args[0].is_empty() {
                1
            } else {
                args[0].parse::<usize>().unwrap_or(1)
            };
            sb.insert_chars(n);
        }
        'P' => {
            // Delete Characters - CSI Ps P
            let n = if args.is_empty() || args[0].is_empty() {
                1
            } else {
                args[0].parse::<usize>().unwrap_or(1)
            };
            sb.delete_chars(n);
        }
        'X' => {
            // Erase Characters - CSI Ps X
            let n = if args.is_empty() || args[0].is_empty() {
                1
            } else {
                args[0].parse::<usize>().unwrap_or(1)
            };
            sb.erase_chars(n);
        }
        'b' => {
            // REP (Repeat) - CSI Ps b
            // Repeat the preceding graphic character Ps times
            let n = if args.is_empty() || args[0].is_empty() {
                1
            } else {
                args[0].parse::<usize>().unwrap_or(1)
            };
            sb.repeat_last_char(n);
        }
        'g' => {
            // TBC (Tab Clear) - CSI Ps g
            // Ps = 0: Clear tab stop at current column
            // Ps = 3: Clear all tab stops
            let mode = if args.is_empty() || args[0].is_empty() {
                0
            } else {
                args[0].parse::<usize>().unwrap_or(0)
            };
            sb.clear_tab_stop(mode);
        }
        'd' => {
            // VPA (Vertical Position Absolute) - CSI Ps d
            let row = if args.is_empty() || args[0].is_empty() {
                1
            } else {
                args[0].parse::<usize>().unwrap_or(1)
            };
            sb.cursor_y = row.saturating_sub(1).min(sb.height() - 1);
        }
        'm' => {
            // SGR (Select Graphic Rendition) - colors and text attributes
            let ([fg, bg], attrs) = ansi::parse_m(sequence);
            if let Some(color) = fg {
                sb.fg_color = color;
            }
            if let Some(color) = bg {
                sb.bg_color = color;
            }
            if let Some(attributes) = attrs {
                sb.bold = attributes.bold;
                sb.italic = attributes.italic;
                sb.underline = attributes.underline;
                sb.strikethrough = attributes.strikethrough;
                sb.blink = attributes.blink;
                sb.reverse = attributes.reverse;
                sb.invisible = attributes.invisible;
            }
        }
        'n' => {
            // Device Status Report (DSR)
            // Check if this is a DEC private mode query with '?' prefix
            if args_str.starts_with('?') {
                let param_str = args_str.trim_start_matches('?');
                let param = if param_str.is_empty() { 0 } else { param_str.parse::<u32>().unwrap_or(0) };

                match param {
                    6 => {
                        // CSI ? 6 n - Report Cursor Position (with '?' prefix)
                        let row = sb.cursor_y + 1; // 1-based
                        let col = sb.cursor_x + 1; // 1-based
                        let response = format!("\x1b[?{};{}R", row, col);
                        if let Ok(mut w) = writer.lock() {
                            let _ = w.write_all(response.as_bytes());
                            let _ = w.flush();
                        }
                    }
                    15 => {
                        // CSI ? 15 n - Report Printer status
                        // Response: CSI ? 13 n (no printer)
                        let response = "\x1b[?13n";
                        if let Ok(mut w) = writer.lock() {
                            let _ = w.write_all(response.as_bytes());
                            let _ = w.flush();
                        }
                    }
                    25 => {
                        // CSI ? 25 n - Report UDK status
                        // Response: CSI ? 21 n (UDK locked)
                        let response = "\x1b[?21n";
                        if let Ok(mut w) = writer.lock() {
                            let _ = w.write_all(response.as_bytes());
                            let _ = w.flush();
                        }
                    }
                    26 => {
                        // CSI ? 26 n - Report Keyboard status
                        // Response: CSI ? 27 ; 1 n (North American keyboard)
                        let response = "\x1b[?27;1n";
                        if let Ok(mut w) = writer.lock() {
                            let _ = w.write_all(response.as_bytes());
                            let _ = w.flush();
                        }
                    }
                    53 => {
                        // CSI ? 53 n - Report Locator status
                        // Response: CSI ? 53 n (no locator)
                        let response = "\x1b[?53n";
                        if let Ok(mut w) = writer.lock() {
                            let _ = w.write_all(response.as_bytes());
                            let _ = w.flush();
                        }
                    }
                    _ => {
                        if debug {
                            eprintln!("[DSR] Ignoring unknown DEC private DSR query (param={})", param);
                        }
                    }
                }
            } else {
                // Standard DSR
                let param = if args.is_empty() || args[0].is_empty() {
                    0
                } else {
                    args[0].parse::<u32>().unwrap_or(0)
                };

                match param {
                    5 => {
                        // CSI 5 n - Status Report
                        // Response: CSI 0 n (terminal OK)
                        let response = "\x1b[0n";
                        if let Ok(mut w) = writer.lock() {
                            if let Err(e) = w.write_all(response.as_bytes()) {
                                eprintln!("[DSR] Failed to send status report: {}", e);
                            } else if let Err(e) = w.flush() {
                                eprintln!("[DSR] Failed to flush status report: {}", e);
                            }
                        }
                    }
                    6 => {
                        // Cursor Position Report (CPR)
                        let row = sb.cursor_y + 1; // 1-based
                        let col = sb.cursor_x + 1; // 1-based
                        let response = format!("\x1b[{};{}R", row, col);

                        // Send response back through PTY to the application
                        if let Ok(mut w) = writer.lock() {
                            if let Err(e) = w.write_all(response.as_bytes()) {
                                eprintln!("[DSR] Failed to send cursor position report: {}", e);
                            } else if let Err(e) = w.flush() {
                                eprintln!("[DSR] Failed to flush cursor position report: {}", e);
                            }
                        } else {
                            eprintln!("[DSR] Failed to acquire writer lock");
                        }
                    }
                    _ => {
                        if debug {
                            eprintln!("[DSR] Ignoring unknown DSR query (param={})", param);
                        }
                    }
                }
            }
        }
        'c' => {
            // Device Attributes (DA)
            // Primary DA: CSI c or CSI 0 c - respond with terminal capabilities
            // Secondary DA: CSI > Ps c - respond with terminal version

            if args.is_empty() || (args.len() == 1 && (args[0].is_empty() || args[0] == "0")) {
                // Primary DA - identify as VT102 compatible
                // Response: CSI ? 6 c (VT102)
                let response = "\x1b[?6c";
                if let Ok(mut w) = writer.lock() {
                    if let Err(e) = w.write_all(response.as_bytes()) {
                        eprintln!("[DA] Failed to send device attributes: {}", e);
                    } else if let Err(e) = w.flush() {
                        eprintln!("[DA] Failed to flush device attributes: {}", e);
                    }
                }
            } else if args.len() == 1 && args[0].starts_with('>') {
                // Secondary DA - respond with terminal type and version
                // Response: CSI > 0 ; 0 ; 0 c (generic terminal)
                let response = "\x1b[>0;0;0c";
                if let Ok(mut w) = writer.lock() {
                    if let Err(e) = w.write_all(response.as_bytes()) {
                        eprintln!("[DA] Failed to send secondary device attributes: {}", e);
                    } else if let Err(e) = w.flush() {
                        eprintln!("[DA] Failed to flush secondary device attributes: {}", e);
                    }
                }
            }
        }
        's' => {
            // Save cursor position (ANSI.SYS style)
            sb.save_cursor();
        }
        'u' => {
            // Restore cursor position (ANSI.SYS style)
            sb.restore_cursor();
        }
        'q' => {
            // DECSCUSR (Set Cursor Style) - CSI Ps SP q
            // The parameter Ps determines the cursor style
            let param = if args.is_empty() || args[0].is_empty() {
                0
            } else {
                // DECSCUSR uses a space before 'q', so trim it
                args[0].trim().parse::<u32>().unwrap_or(0)
            };

            use crate::screen_buffer::CursorStyle;
            sb.cursor_style = match param {
                1 => CursorStyle::BlinkingBlock,
                2 => CursorStyle::SteadyBlock,
                3 => CursorStyle::BlinkingUnderline,
                4 => CursorStyle::SteadyUnderline,
                5 => CursorStyle::BlinkingBar,
                6 => CursorStyle::SteadyBar,
                _ => CursorStyle::BlinkingBlock, // 0 or unknown defaults to blinking block
            };
        }
        'Z' => {
            // CBT (Cursor Backward Tabulation)
            let n = if args.is_empty() || args[0].is_empty() {
                1
            } else {
                args[0].parse::<usize>().unwrap_or(1)
            };
            sb.back_tab(n);
        }
        'a' => {
            // HPR (Horizontal Position Relative)
            let n = if args.is_empty() || args[0].is_empty() {
                1
            } else {
                args[0].parse::<usize>().unwrap_or(1)
            };
            sb.move_cursor_right(n);
        }
        'e' => {
            // VPR (Vertical Position Relative)
            let n = if args.is_empty() || args[0].is_empty() {
                1
            } else {
                args[0].parse::<usize>().unwrap_or(1)
            };
            sb.move_cursor_down(n);
        }
        _ => {
            if debug {
                eprintln!("[TERMINAL] Ignoring unknown CSI sequence: {:?}", sequence);
            }
        }
    }
}

// Parse mode sequences like "?1049h", "?1049l", "?25h", "?25l"
fn parse_mode_sequences_old(sequence: &str, debug: bool) -> Vec<String> {
    let bytes = sequence.as_bytes();
    let mut mode_numbers = Vec::new();
    let mut i = 0;

    if debug {
        eprintln!("[TERMINAL] Parsing mode sequence: {:?}", sequence);
    }

    while i < bytes.len() {
        // Skip non-digit characters until we find a digit or reach end
        while i < bytes.len() && !bytes[i].is_ascii_digit() && bytes[i] != b'?' {
            i += 1;
        }

        if i >= bytes.len() {
            break;
        }

        // Handle optional '?' prefix
        let mut mode_str = String::new();
        if i < bytes.len() && bytes[i] == b'?' {
            mode_str.push('?');
            i += 1;
        }

        // Collect digits
        loop {
            let mut num_str = String::new();
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                num_str.push(bytes[i] as char);
                i += 1;
            }

            if !num_str.is_empty() {
                mode_numbers.push(mode_str.clone() + &num_str);
            }

            // Check for semicolon separator
            if i < bytes.len() && bytes[i] == b';' {
                i += 1; // Skip semicolon
                continue;
            } else {
                break;
            }
        }
    }

    if debug {
        eprintln!("[TERMINAL] Parsed mode numbers: {:?}", mode_numbers);
    }

    mode_numbers
}
