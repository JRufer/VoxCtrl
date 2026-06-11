use std::io::BufRead;
use std::sync::{Arc, Mutex};
use slint::ComponentHandle;

slint::slint! {
    export struct BarData {
        height: float,
        opacity: float,
    }

    export component OverlayWindow inherits Window {
        // Window properties
        always-on-top: true;
        no-frame: true;
        background: transparent;
        width: 560px;
        height: 190px; // Sized to fit capsule + response box

        // Properties updated by Rust
        in property <bool> is-recording: false;
        in property <bool> is-processing: false;
        in property <bool> is-speaking: false;
        in property <bool> is-audio-ready: true;
        in property <string> target-label: "Focused Window";
        in property <[BarData]> bars;
        in property <string> overlay-style: "waveform";

        // Visual Layout
        VerticalLayout {
            padding: 0px;
            spacing: 12px;
            alignment: start;

            // ─── 1. WAVEFORM STYLE ───
            if (overlay-style == "waveform" && (is-recording || is-processing)) : Rectangle {
                width: 540px;
                height: 110px;
                background: @linear-gradient(160deg, #0d0e12 0%, #08090b 100%);
                border-width: 1px;
                border-color: rgba(255, 255, 255, 0.08);
                border-radius: 12px;

                VerticalLayout {
                    padding-left: 20px;
                    padding-right: 20px;
                    padding-top: 14px;
                    padding-bottom: 16px;
                    spacing: 12px;

                    // Info bar
                    HorizontalLayout {
                        alignment: space-between;
                        height: 20px;

                        // Left status labels
                        HorizontalLayout {
                            alignment: start;
                            spacing: 8px;

                            // Glowing dot indicator
                            Rectangle {
                                width: 6px;
                                height: 6px;
                                border-radius: 3px;
                                y: (parent.height - self.height) / 2;
                                background: is-processing ? #38bdf8 : (!is-audio-ready ? #f59e0b : #ff6b35);
                            }

                            Text {
                                text: is-processing ? "PROCESSING..." : "🎙️ MIC";
                                color: #ffffff;
                                font-size: 10px;
                                font-weight: 800;
                                font-family: "Outfit";
                            }

                            Text {
                                text: "·";
                                color: rgba(255, 255, 255, 0.15);
                                font-size: 10px;
                            }

                            Text {
                                text: is-processing ? "thinking with AI" : (!is-audio-ready ? "preparing stream" : "voice overlay");
                                color: rgba(255, 255, 255, 0.3);
                                font-size: 10px;
                                font-weight: 500;
                                font-family: "Outfit";
                            }
                        }

                        // Right active target pill
                        Rectangle {
                            background: rgba(255, 255, 255, 0.04);
                            border-width: 1px;
                            border-color: rgba(255, 255, 255, 0.08);
                            border-radius: 99px;

                            HorizontalLayout {
                                spacing: 6px;
                                alignment: center;
                                padding-left: 12px;
                                padding-right: 12px;
                                padding-top: 2.5px;
                                padding-bottom: 2.5px;

                                Text {
                                    text: "🎯";
                                    font-size: 10px;
                                }
                                Text {
                                    text: target-label;
                                    color: #c5c6c9;
                                    font-size: 9px;
                                    font-weight: 800;
                                    font-family: "Outfit";
                                }
                            }
                        }
                    }

                    // Waveform stage
                    Rectangle {
                        height: 70px;

                        // Center axis line
                        Rectangle {
                            y: parent.height / 2;
                            height: 1px;
                            background: rgba(255, 255, 255, 0.06);
                        }

                        // Center-aligned animated bars
                        HorizontalLayout {
                            alignment: center;
                            spacing: 3px;
                            width: 100%;
                            height: 100%;

                            for bar in bars : Rectangle {
                                width: 3px;
                                height: bar.height * 1px;
                                background: is-processing ? #38bdf8 : #ff6b35;
                                border-radius: 1.5px;
                                opacity: bar.opacity;
                            }
                        }
                    }
                }
            }

            // ─── 2. VOICE CARD STYLE ───
            if (overlay-style == "voice_card" && (is-recording || is-processing)) : Rectangle {
                width: 320px;
                height: 120px;
                background: rgba(18, 18, 22, 0.94);
                border-width: 1.2px;
                border-color: is-processing ? rgba(6, 182, 212, 0.35) : rgba(255, 255, 255, 0.05);
                border-radius: 28px;
                x: 120px; // Center in the 560px window ((560 - 320)/2)

                VerticalLayout {
                    padding: 14px;
                    spacing: 8px;

                    // Header row
                    HorizontalLayout {
                        alignment: space-between;
                        height: 20px;

                        Text {
                            text: is-processing ? "AI Thinking" : "Voice Activity";
                            color: rgba(255, 255, 255, 0.35);
                            font-size: 11px;
                            font-weight: 600;
                            font-family: "Inter";
                        }

                        Rectangle {
                            background: is-processing ? rgba(6, 182, 212, 0.07) : rgba(16, 185, 129, 0.07);
                            border-width: 1px;
                            border-color: is-processing ? rgba(6, 182, 212, 0.4) : rgba(16, 185, 129, 0.4);
                            border-radius: 6px;

                            HorizontalLayout {
                                padding-left: 9px;
                                padding-right: 9px;
                                padding-top: 2.5px;
                                padding-bottom: 2.5px;
                                alignment: center;

                                Text {
                                    text: is-processing ? "Processing..." : target-label;
                                    color: is-processing ? #06b6d4 : #10b981;
                                    font-size: 10.5px;
                                    font-weight: 600;
                                    font-family: "Inter";
                                }
                            }
                        }
                    }

                    // Equalizer bars
                    HorizontalLayout {
                        alignment: center;
                        spacing: 2px;
                        height: 60px;

                        for bar in bars : Rectangle {
                            width: 3.5px;
                            height: bar.height * 0.75 * 1px; // Scale slightly for card
                            // Approximates purple/pink/red HSL hues from Svelte code
                            background: is-processing ? #06b6d4 : #a855f7;
                            border-radius: 99px;
                            opacity: bar.opacity;
                        }
                    }
                }
            }

            // ─── 3. BLUE WAVE STYLE ───
            if (overlay-style == "blue_wave" && (is-recording || is-processing)) : Rectangle {
                width: 320px;
                height: 120px;
                background: rgba(18, 18, 22, 0.96);
                border-width: 1.2px;
                border-color: is-processing ? rgba(6, 182, 212, 0.35) : rgba(255, 255, 255, 0.05);
                border-radius: 28px;
                x: 120px; // Center in the 560px window

                VerticalLayout {
                    padding: 14px;
                    spacing: 8px;

                    // Header row
                    HorizontalLayout {
                        alignment: space-between;
                        height: 20px;

                        Text {
                            text: is-processing ? "AI Thinking" : "Voice Activity";
                            color: rgba(255, 255, 255, 0.35);
                            font-size: 11px;
                            font-weight: 600;
                            font-family: "Inter";
                        }

                        Rectangle {
                            background: is-processing ? rgba(6, 182, 212, 0.07) : rgba(16, 185, 129, 0.07);
                            border-width: 1px;
                            border-color: is-processing ? rgba(6, 182, 212, 0.4) : rgba(16, 185, 129, 0.4);
                            border-radius: 6px;

                            HorizontalLayout {
                                padding-left: 9px;
                                padding-right: 9px;
                                padding-top: 2.5px;
                                padding-bottom: 2.5px;
                                alignment: center;

                                Text {
                                    text: is-processing ? "Processing..." : target-label;
                                    color: is-processing ? #06b6d4 : #10b981;
                                    font-size: 10.5px;
                                    font-weight: 600;
                                    font-family: "Inter";
                                }
                            }
                        }
                    }

                    // Ocean fluid waves (rendered as cyan vertical bars)
                    HorizontalLayout {
                        alignment: center;
                        spacing: 2px;
                        height: 60px;

                        for bar in bars : Rectangle {
                            width: 3.5px;
                            height: bar.height * 0.75 * 1px;
                            background: #22d3ee; // Aqua Cyan
                            border-radius: 99px;
                            opacity: bar.opacity;
                        }
                    }
                }
            }

            // ─── 4. PULSE STYLE ───
            if (overlay-style == "pulse" && (is-recording || is-processing)) : Rectangle {
                width: 340px;
                height: 56px;
                background: rgba(13, 14, 18, 0.92);
                border-width: 1px;
                border-color: rgba(255, 255, 255, 0.06);
                border-radius: 28px;
                x: 110px; // Center in the 560px window ((560 - 340)/2)

                HorizontalLayout {
                    padding-left: 6px;
                    padding-right: 16px;
                    padding-top: 6px;
                    padding-bottom: 6px;
                    spacing: 14px;
                    alignment: start;

                    // Core pulsing indicator
                    Rectangle {
                        width: 44px;
                        height: 44px;
                        border-radius: 22px;
                        background: transparent;

                        Rectangle {
                            width: 14px;
                            height: 14px;
                            border-radius: 7px;
                            background: is-processing ? #38bdf8 : (!is-audio-ready ? #f59e0b : #ff6b35);
                            x: 15px;
                            y: 15px;
                        }
                    }

                    // Target badge
                    Rectangle {
                        background: rgba(255, 255, 255, 0.04);
                        border-width: 1px;
                        border-color: is-processing ? rgba(56, 189, 248, 0.25) : (!is-audio-ready ? rgba(245, 158, 11, 0.25) : rgba(255, 107, 53, 0.25));
                        border-radius: 99px;
                        y: (parent.height - self.height) / 2;

                        HorizontalLayout {
                            padding-left: 10px;
                            padding-right: 10px;
                            padding-top: 3px;
                            padding-bottom: 3px;
                            spacing: 5px;
                            alignment: center;

                            Text {
                                text: is-processing ? "🧠" : (!is-audio-ready ? "⏳" : "🎯");
                                font-size: 11px;
                            }

                            Text {
                                text: is-processing ? "PROCESSING..." : (!is-audio-ready ? "CONNECTING..." : target-label);
                                color: is-processing ? #38bdf8 : (!is-audio-ready ? #f59e0b : #ff6b35);
                                font-size: 9.5px;
                                font-weight: 850;
                                font-family: "Outfit";
                            }
                        }
                    }
                }
            }

            // Floating System Response Pill (appears underneath capsule)
            if (is-speaking) : Rectangle {
                width: 260px;
                height: 54px;
                background: rgba(239, 68, 68, 0.88);
                border-width: 2px;
                border-color: rgba(239, 68, 68, 0.85);
                border-radius: 12px;
                x: 150px; // Center dynamically (560 - 260) / 2

                HorizontalLayout {
                    alignment: center;
                    spacing: 12px;

                    // Flashing dot
                    Rectangle {
                        width: 10px;
                        height: 10px;
                        border-radius: 5px;
                        background: #ffffff;
                        y: (parent.height - self.height) / 2;
                    }

                    Text {
                        text: "SYSTEM RESPONDING...";
                        color: #ffffff;
                        font-size: 13px;
                        font-weight: 700;
                        font-family: "Inter";
                    }
                }
            }
        }
    }
}

struct AppState {
    recording: bool,
    processing: bool,
    speaking: bool,
    audio_ready: bool,
    audio_level: f32,
    active_target_label: String,
    x: i32,
    y: i32,
    overlay_style: String,
}

fn main() {
    let ui = OverlayWindow::new().unwrap();
    
    // Start with the window hidden. We use show()/hide() dynamically.

    let shared_state = Arc::new(Mutex::new(AppState {
        recording: false,
        processing: false,
        speaking: false,
        audio_ready: true,
        audio_level: 0.0,
        active_target_label: "Focused Window".to_string(),
        x: -20000,
        y: -20000,
        overlay_style: "waveform".to_string(),
    }));
    
    // Stdin reading thread
    let state_clone = Arc::clone(&shared_state);
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut reader = stdin.lock();
        let mut line = String::new();
        
        loop {
            line.clear();
            if reader.read_line(&mut line).is_err() || line.is_empty() {
                // Pipe closed, exit
                slint::invoke_from_event_loop(move || {
                    let _ = slint::quit_event_loop();
                }).unwrap();
                break;
            }
            
            if let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line) {
                eprintln!("[overlay] Received JSON: {}", line.trim());
                if let Some(msg_type) = msg.get("type").and_then(|t| t.as_str()) {
                    match msg_type {
                        "status" => {
                            if let Ok(mut state) = state_clone.lock() {
                                state.recording = msg.get("recording").and_then(|v| v.as_bool()).unwrap_or(false);
                                state.processing = msg.get("processing").and_then(|v| v.as_bool()).unwrap_or(false);
                                state.speaking = msg.get("speaking").and_then(|v| v.as_bool()).unwrap_or(false);
                                state.audio_ready = msg.get("audio_ready").and_then(|v| v.as_bool()).unwrap_or(true);
                                state.audio_level = msg.get("audio_level").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                                if let Some(target) = msg.get("active_target_label").and_then(|v| v.as_str()) {
                                    state.active_target_label = target.to_string();
                                }
                                if let Some(style) = msg.get("overlay_style").and_then(|v| v.as_str()) {
                                    state.overlay_style = style.to_string();
                                }
                            }
                        }
                        "position" => {
                            if let Ok(mut state) = state_clone.lock() {
                                state.x = msg.get("x").and_then(|v| v.as_i64()).unwrap_or(-20000) as i32;
                                state.y = msg.get("y").and_then(|v| v.as_i64()).unwrap_or(-20000) as i32;
                            }
                        }
                        "shutdown" => {
                            slint::invoke_from_event_loop(move || {
                                let _ = slint::quit_event_loop();
                            }).unwrap();
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }
    });

    let timer = slint::Timer::default();
    let state_clone2 = Arc::clone(&shared_state);
    let ui_weak = ui.as_weak();

    // Gaussian envelope precalculation
    let mut envelope = [0.0f32; 48];
    let bar_count = 48;
    let mid = (bar_count as f32 - 1.0) / 2.0;
    let sigma = bar_count as f32 / 5.0;
    for i in 0..bar_count {
        envelope[i] = ( -((i as f32 - mid).powi(2)) / (2.0 * sigma.powi(2)) ).exp();
    }

    let mut offset = 0.0f32;
    let mut lcg_state = 12345u32;
    let mut last_pos = (-99999, -99999, false);

    timer.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(16), move || {
        let ui = match ui_weak.upgrade() {
            Some(ui) => ui,
            None => return,
        };

        let (rec, proc, speak, ready, level, target, tx, ty, style) = {
            if let Ok(s) = state_clone2.lock() {
                (s.recording, s.processing, s.speaking, s.audio_ready, s.audio_level, s.active_target_label.clone(), s.x, s.y, s.overlay_style.clone())
            } else {
                return;
            }
        };

        // Window positioning and visibility
        let active = rec || proc || speak;
        if last_pos != (tx, ty, active) {
            let old_active = last_pos.2;
            last_pos = (tx, ty, active);
            eprintln!("[overlay] State update: x={}, y={}, active={}", tx, ty, active);
            
            if active {
                if !old_active {
                    if let Err(e) = ui.show() {
                        eprintln!("[overlay] Failed to show window: {:?}", e);
                    }
                }
            } else {
                if let Err(e) = ui.hide() {
                    eprintln!("[overlay] Failed to hide window: {:?}", e);
                }
            }
        }

        if active {
            ui.window().set_position(slint::PhysicalPosition::new(tx, ty));
        }

        ui.set_is_recording(rec);
        ui.set_is_processing(proc);
        ui.set_is_speaking(speak);
        ui.set_is_audio_ready(ready);
        ui.set_target_label(target.into());
        ui.set_overlay_style(style.clone().into());

        // Update waveform bars model
        let bars = std::rc::Rc::new(slint::VecModel::default());
        let num_bars = if style == "waveform" { 48 } else { 45 };

        if rec {
            offset = 0.0;
            for i in 0..num_bars {
                let env = envelope[i];
                let base = 2.0 + env * 8.0;
                
                // LCG random value between 0.0 and 1.0
                lcg_state = lcg_state.wrapping_mul(1103515245).wrapping_add(12345);
                let rand_val = (lcg_state as f32) / (u32::MAX as f32);
                
                let noise = rand_val * env * level * 56.0;
                let height = base + noise;
                let opacity = if ready { 0.45 + env * 0.55 } else { 0.18 };
                bars.push(BarData { height, opacity });
            }
        } else if proc {
            offset += 0.22;
            for i in 0..num_bars {
                let env = envelope[i];
                let base = 2.0 + env * 4.0;
                let wave = (i as f32 * 0.45 - offset).sin() * env * 28.0;
                let height = base + wave.abs();
                let opacity = 0.45 + env * 0.55;
                bars.push(BarData { height, opacity });
            }
        } else {
            // Flat line
            for _ in 0..num_bars {
                bars.push(BarData { height: 2.0, opacity: 0.18 });
            }
        }

        ui.set_bars(bars.into());
    });

    slint::run_event_loop_until_quit().unwrap();
}
