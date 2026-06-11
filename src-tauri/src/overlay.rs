use std::collections::VecDeque;
use std::fmt::Write as _;
use std::io::BufRead;
use std::sync::{Arc, Mutex};
use slint::ComponentHandle;

slint::slint! {
    export struct BubbleData {
        y: float,
        o: float,
    }

    export component OverlayWindow inherits Window {
        // Window properties
        always-on-top: true;
        no-frame: true;
        background: transparent;
        width: 560px;
        height: 190px;

        // Status properties updated by Rust
        in property <bool> is-recording: false;
        in property <bool> is-processing: false;
        in property <bool> is-speaking: false;
        in property <bool> is-audio-ready: true;
        in property <string> target-label: "Focused Window";
        in property <string> overlay-style: "waveform";

        // Animation properties driven by the Rust spring/timer loop
        in property <float> reveal-main: 0.0;   // 0..1 load/unload progress for the visualizer
        in property <float> reveal-pill: 0.0;   // 0..1 load/unload progress for the speaking pill
        in property <float> level: 0.0;         // smoothed audio level 0..1
        in property <float> blink: 1.0;         // 0..1 pulsing value
        in property <float> sheen-x: -70.0;     // holo sheen x position (voice card)
        in property <float> buoy-y: 40.0;       // floating buoy y position (ocean)

        // Pre-built vector path data (computed in Rust each tick)
        in property <string> osc-path: "";
        in property <string> ocean-path1: "";
        in property <string> ocean-path2: "";
        in property <string> ocean-path3: "";
        in property <string> sweep-path: "";
        in property <string> sweep-trail-path: "";

        // Radar pulse rings + blips
        in property <float> ring1-s: 24.0;
        in property <float> ring1-o: 0.0;
        in property <float> ring2-s: 24.0;
        in property <float> ring2-o: 0.0;
        in property <float> blip1-o: 0.0;
        in property <float> blip2-o: 0.0;

        // LED matrix column levels (voice card) and speaking pill mini-equalizer
        in property <[float]> led-cols: [];
        in property <[float]> pill-bars: [];
        in property <[BubbleData]> bubbles: [];

        // ─────────────────────────────────────────────────────────────
        // 1. WAVEFORM — "OSC-01" green phosphor oscilloscope.
        //    Loads like a CRT powering on (expands from a scanline),
        //    unloads by collapsing back into the line.
        // ─────────────────────────────────────────────────────────────
        if (overlay-style == "waveform" && reveal-main > 0.004) : Rectangle {
            x: 10px;
            y: 14px + (124px * (1.0 - Math.min(1.0, reveal-main))) / 2;
            width: 540px;
            height: 124px * reveal-main;
            clip: true;
            background: @linear-gradient(180deg, #07140c 0%, #030a07 100%);
            border-width: 1px;
            border-color: rgba(74, 222, 128, 0.25);
            border-radius: 10px;
            opacity: reveal-main > 0.33 ? 1.0 : reveal-main * 3.0;
            drop-shadow-blur: 18px;
            drop-shadow-color: rgba(34, 197, 94, 0.18);

            // Fixed-size inner content, vertically anchored so the
            // collapse reads as a CRT switching off.
            Rectangle {
                x: 0;
                y: (parent.height - 124px) / 2;
                width: 540px;
                height: 124px;

                // Header row
                HorizontalLayout {
                    x: 20px;
                    y: 10px;
                    width: 500px;
                    height: 18px;
                    spacing: 8px;

                    Rectangle {
                        width: 7px;
                        height: 7px;
                        border-radius: 3.5px;
                        y: (parent.height - self.height) / 2;
                        background: is-processing ? #38bdf8 : (!is-audio-ready ? #f59e0b : #4ade80);
                        drop-shadow-blur: 6px;
                        drop-shadow-color: is-processing ? #38bdf8 : (!is-audio-ready ? #f59e0b : #4ade80);
                        opacity: 0.4 + 0.6 * blink;
                    }

                    Text {
                        text: "WAVEFORM // OSC-01";
                        color: #bbf7d0;
                        font-size: 10px;
                        font-weight: 800;
                        font-family: "Outfit";
                        letter-spacing: 1.2px;
                        vertical-alignment: center;
                    }

                    Text {
                        text: is-processing ? "· TRANSCRIBING" : (!is-audio-ready ? "· CALIBRATING" : "· LIVE TRACE");
                        color: rgba(187, 247, 208, 0.35);
                        font-size: 9px;
                        font-weight: 500;
                        font-family: "Outfit";
                        letter-spacing: 1px;
                        vertical-alignment: center;
                    }

                    Rectangle { horizontal-stretch: 1; }

                    // Target readout chip
                    Rectangle {
                        horizontal-stretch: 0;
                        background: rgba(74, 222, 128, 0.06);
                        border-width: 1px;
                        border-color: rgba(74, 222, 128, 0.3);
                        border-radius: 4px;

                        HorizontalLayout {
                            padding-left: 8px;
                            padding-right: 8px;
                            padding-top: 2px;
                            padding-bottom: 2px;
                            spacing: 5px;
                            alignment: center;

                            Text {
                                text: "TGT ▸";
                                color: #4ade80;
                                font-size: 8.5px;
                                font-weight: 800;
                                font-family: "Outfit";
                                letter-spacing: 1px;
                                vertical-alignment: center;
                            }
                            Text {
                                text: target-label;
                                color: #d1fae5;
                                font-size: 8.5px;
                                font-weight: 700;
                                font-family: "Outfit";
                                letter-spacing: 0.6px;
                                vertical-alignment: center;
                                overflow: elide;
                                max-width: 150px;
                            }
                        }
                    }
                }

                // Scope stage
                Rectangle {
                    x: 20px;
                    y: 36px;
                    width: 500px;
                    height: 78px;

                    // Horizontal graticule lines
                    Rectangle { x: 0; y: 19px; width: 500px; height: 1px; background: rgba(74, 222, 128, 0.07); }
                    Rectangle { x: 0; y: 39px; width: 500px; height: 1px; background: rgba(74, 222, 128, 0.14); }
                    Rectangle { x: 0; y: 59px; width: 500px; height: 1px; background: rgba(74, 222, 128, 0.07); }

                    // Vertical graticule ticks
                    for t in [0, 1, 2, 3, 4, 5] : Rectangle {
                        x: t * 100px;
                        y: 0;
                        width: 1px;
                        height: 78px;
                        background: rgba(74, 222, 128, 0.05);
                    }

                    // Phosphor glow pass
                    Path {
                        x: 0; y: 0;
                        width: 500px;
                        height: 78px;
                        viewbox-width: 500;
                        viewbox-height: 78;
                        commands: osc-path;
                        fill: transparent;
                        stroke: is-processing ? #38bdf8 : (!is-audio-ready ? #f59e0b : #4ade80);
                        stroke-width: 6px;
                        opacity: 0.16;
                    }

                    // Crisp trace pass
                    Path {
                        x: 0; y: 0;
                        width: 500px;
                        height: 78px;
                        viewbox-width: 500;
                        viewbox-height: 78;
                        commands: osc-path;
                        fill: transparent;
                        stroke: is-processing ? #7dd3fc : (!is-audio-ready ? #fcd34d : #a7f3d0);
                        stroke-width: 1.8px;
                    }
                }
            }
        }

        // ─────────────────────────────────────────────────────────────
        // 2. PULSE RING — sonar/radar dial with rotating sweep,
        //    expanding pulse rings and a target-lock side plate.
        //    Loads by dropping/fading the dial in, plate slides out.
        // ─────────────────────────────────────────────────────────────
        if (overlay-style == "pulse" && reveal-main > 0.004) : Rectangle {
            x: 0;
            y: 0;
            width: 560px;
            height: 190px;
            opacity: Math.min(1.0, reveal-main);

            // Radar dial
            Rectangle {
                x: 142px;
                y: 24px + (1.0 - reveal-main) * 30px;
                width: 124px;
                height: 124px;
                border-radius: 62px;
                clip: true;
                background: @radial-gradient(circle, #10181c 0%, #06090b 85%, #04070a 100%);
                border-width: 1.2px;
                border-color: is-processing ? rgba(56, 189, 248, 0.4) : (!is-audio-ready ? rgba(245, 158, 11, 0.4) : rgba(255, 107, 53, 0.4));
                drop-shadow-blur: 22px;
                drop-shadow-color: is-processing ? rgba(56, 189, 248, 0.2) : rgba(255, 107, 53, 0.2);

                // Static range rings
                Rectangle {
                    x: (124px - self.width) / 2;
                    y: (124px - self.height) / 2;
                    width: 92px;
                    height: 92px;
                    border-radius: 46px;
                    border-width: 1px;
                    border-color: rgba(255, 255, 255, 0.10);
                }
                Rectangle {
                    x: (124px - self.width) / 2;
                    y: (124px - self.height) / 2;
                    width: 56px;
                    height: 56px;
                    border-radius: 28px;
                    border-width: 1px;
                    border-color: rgba(255, 255, 255, 0.14);
                }

                // Crosshair axes
                Rectangle { x: 0; y: 61.5px; width: 124px; height: 1px; background: rgba(255, 255, 255, 0.07); }
                Rectangle { x: 61.5px; y: 0; width: 1px; height: 124px; background: rgba(255, 255, 255, 0.07); }

                // Sweep trail (faded) and sweep arm
                Path {
                    x: 0; y: 0; width: 124px; height: 124px;
                    viewbox-width: 124;
                    viewbox-height: 124;
                    commands: sweep-trail-path;
                    fill: transparent;
                    stroke: is-processing ? rgba(56, 189, 248, 0.18) : rgba(255, 107, 53, 0.18);
                    stroke-width: 2.5px;
                }
                Path {
                    x: 0; y: 0; width: 124px; height: 124px;
                    viewbox-width: 124;
                    viewbox-height: 124;
                    commands: sweep-path;
                    fill: transparent;
                    stroke: is-processing ? #38bdf8 : (!is-audio-ready ? #f59e0b : #ff6b35);
                    stroke-width: 2px;
                }

                // Expanding audio pulse rings
                Rectangle {
                    x: (124px - self.width) / 2;
                    y: (124px - self.height) / 2;
                    width: ring1-s * 1px;
                    height: ring1-s * 1px;
                    border-radius: self.width / 2;
                    border-width: 1.5px;
                    border-color: is-processing ? #38bdf8 : (!is-audio-ready ? #f59e0b : #ff6b35);
                    opacity: ring1-o;
                }
                Rectangle {
                    x: (124px - self.width) / 2;
                    y: (124px - self.height) / 2;
                    width: ring2-s * 1px;
                    height: ring2-s * 1px;
                    border-radius: self.width / 2;
                    border-width: 1.5px;
                    border-color: is-processing ? #38bdf8 : (!is-audio-ready ? #f59e0b : #ff6b35);
                    opacity: ring2-o;
                }

                // Contact blips lit up by the passing sweep
                Rectangle {
                    x: 88px; y: 42px; width: 5px; height: 5px; border-radius: 2.5px;
                    background: is-processing ? #7dd3fc : #ffb38a;
                    opacity: blip1-o;
                }
                Rectangle {
                    x: 36px; y: 86px; width: 4px; height: 4px; border-radius: 2px;
                    background: is-processing ? #7dd3fc : #ffb38a;
                    opacity: blip2-o;
                }

                // Audio-reactive core
                Rectangle {
                    x: (124px - self.width) / 2;
                    y: (124px - self.height) / 2;
                    width: (11.0 + level * 18.0) * 1px;
                    height: self.width;
                    border-radius: self.width / 2;
                    background: is-processing ? #38bdf8 : (!is-audio-ready ? #f59e0b : #ff6b35);
                    drop-shadow-blur: 14px;
                    drop-shadow-color: is-processing ? #38bdf8 : (!is-audio-ready ? #f59e0b : #ff6b35);
                }
            }

            // Target-lock plate
            Rectangle {
                x: 290px + (1.0 - reveal-main) * 22px;
                y: 60px;
                width: 200px;
                height: 52px;
                background: rgba(10, 12, 16, 0.94);
                border-width: 1px;
                border-color: rgba(255, 255, 255, 0.08);
                border-radius: 10px;
                opacity: Math.min(1.0, reveal-main);

                // Pulsing lock frame
                Rectangle {
                    x: 0; y: 0; width: 200px; height: 52px;
                    border-radius: 10px;
                    border-width: 1px;
                    border-color: is-processing ? rgba(56, 189, 248, 0.55) : (!is-audio-ready ? rgba(245, 158, 11, 0.55) : rgba(255, 107, 53, 0.55));
                    opacity: 0.25 + 0.75 * blink;
                }

                HorizontalLayout {
                    padding-left: 12px;
                    padding-right: 12px;
                    spacing: 10px;

                    Text {
                        text: "⌖";
                        color: is-processing ? #38bdf8 : (!is-audio-ready ? #f59e0b : #ff6b35);
                        font-size: 22px;
                        vertical-alignment: center;
                        opacity: 0.5 + 0.5 * blink;
                    }

                    VerticalLayout {
                        alignment: center;
                        spacing: 2px;

                        Text {
                            text: is-processing ? "PULSE // ANALYZING" : (!is-audio-ready ? "PULSE // ACQUIRING" : "PULSE // TARGET LOCK");
                            color: rgba(255, 255, 255, 0.35);
                            font-size: 7.5px;
                            font-weight: 800;
                            font-family: "Outfit";
                            letter-spacing: 1.4px;
                        }
                        Text {
                            text: is-processing ? "Decoding transmission…" : (!is-audio-ready ? "Connecting mic…" : target-label);
                            color: #e5e7eb;
                            font-size: 12px;
                            font-weight: 750;
                            font-family: "Outfit";
                            overflow: elide;
                            max-width: 150px;
                        }
                    }
                }
            }
        }

        // ─────────────────────────────────────────────────────────────
        // 3. OCEAN WAVE — glass tide pool. The water level rises with
        //    your voice; bubbles drift up and a buoy tag floats on the
        //    surface showing the target. Fills on load, drains on unload.
        // ─────────────────────────────────────────────────────────────
        if (overlay-style == "blue_wave" && reveal-main > 0.004) : Rectangle {
            x: 90px;
            y: 20px + (1.0 - reveal-main) * 16px;
            width: 380px;
            height: 128px;
            border-radius: 24px;
            clip: true;
            background: @linear-gradient(180deg, rgba(8, 18, 32, 0.96) 0%, rgba(3, 9, 18, 0.97) 100%);
            border-width: 1.2px;
            border-color: is-processing ? rgba(56, 189, 248, 0.35) : rgba(34, 211, 238, 0.25);
            opacity: Math.min(1.0, reveal-main);
            drop-shadow-blur: 24px;
            drop-shadow-color: rgba(8, 145, 178, 0.25);

            // Moonlight glow
            Rectangle {
                x: 296px; y: 10px; width: 22px; height: 22px;
                border-radius: 11px;
                background: rgba(224, 242, 254, 0.85);
                drop-shadow-blur: 16px;
                drop-shadow-color: rgba(186, 230, 253, 0.6);
                opacity: 0.55 + 0.2 * blink;
            }

            Text {
                x: 18px;
                y: 12px;
                text: "OCEAN WAVE";
                color: rgba(186, 230, 253, 0.55);
                font-size: 9.5px;
                font-weight: 800;
                font-family: "Outfit";
                letter-spacing: 2px;
            }
            Text {
                x: 18px;
                y: 25px;
                text: is-processing ? "deep current — processing" : (!is-audio-ready ? "low tide — preparing" : "high tide — listening");
                color: rgba(125, 211, 252, 0.35);
                font-size: 8px;
                font-weight: 500;
                font-family: "Outfit";
                letter-spacing: 0.8px;
            }

            // Layered water (paths rebuilt every frame in Rust)
            Path {
                x: 0; y: 38px; width: 380px; height: 90px;
                viewbox-width: 380; viewbox-height: 90;
                commands: ocean-path1;
                fill: is-processing ? rgba(30, 64, 175, 0.40) : rgba(2, 132, 199, 0.35);
                stroke: rgba(2, 132, 199, 0.2);
                stroke-width: 1px;
            }
            Path {
                x: 0; y: 38px; width: 380px; height: 90px;
                viewbox-width: 380; viewbox-height: 90;
                commands: ocean-path2;
                fill: is-processing ? rgba(59, 130, 246, 0.45) : rgba(6, 182, 212, 0.45);
                stroke: rgba(6, 182, 212, 0.25);
                stroke-width: 1px;
            }
            Path {
                x: 0; y: 38px; width: 380px; height: 90px;
                viewbox-width: 380; viewbox-height: 90;
                commands: ocean-path3;
                fill: is-processing ? rgba(96, 165, 250, 0.55) : rgba(34, 211, 238, 0.6);
                stroke: is-processing ? rgba(147, 197, 253, 0.6) : rgba(165, 243, 252, 0.55);
                stroke-width: 1.5px;
            }

            // Rising bubbles
            for b[i] in bubbles : Rectangle {
                x: 64px + i * 112px;
                y: b.y * 1px;
                width: i == 1 ? 6px : 4px;
                height: self.width;
                border-radius: self.width / 2;
                border-width: 1px;
                border-color: rgba(165, 243, 252, 0.7);
                background: rgba(165, 243, 252, 0.12);
                opacity: b.o;
            }

            // Floating buoy target tag (bobs on the surface)
            Rectangle {
                x: (380px - self.width) / 2;
                y: buoy-y * 1px;
                width: 168px;
                height: 24px;
                border-radius: 12px;
                background: rgba(8, 47, 73, 0.92);
                border-width: 1px;
                border-color: rgba(125, 211, 252, 0.5);

                HorizontalLayout {
                    padding-left: 10px;
                    padding-right: 12px;
                    spacing: 6px;
                    alignment: center;

                    Rectangle {
                        width: 7px;
                        height: 7px;
                        border-radius: 3.5px;
                        y: (parent.height - self.height) / 2;
                        background: is-processing ? #60a5fa : (!is-audio-ready ? #f59e0b : #22d3ee);
                        opacity: 0.5 + 0.5 * blink;
                    }
                    Text {
                        text: is-processing ? "Sounding the depths…" : (!is-audio-ready ? "Casting off…" : target-label);
                        color: #e0f2fe;
                        font-size: 10px;
                        font-weight: 700;
                        font-family: "Outfit";
                        vertical-alignment: center;
                        overflow: elide;
                        max-width: 130px;
                    }
                }
            }
        }

        // ─────────────────────────────────────────────────────────────
        // 4. VOICE CARD — a literal membership card: gold chip, holo
        //    sheen, VU-meter LED dot matrix and an embossed target
        //    field. Deals in with a card-flip (horizontal unfold).
        // ─────────────────────────────────────────────────────────────
        if (overlay-style == "voice_card" && reveal-main > 0.004) : Rectangle {
            width: 340px * Math.max(0.001, reveal-main);
            x: (560px - self.width) / 2;
            y: 16px;
            height: 152px;
            clip: true;
            border-radius: 16px;
            background: @linear-gradient(135deg, #181b24 0%, #0b0d12 55%, #141925 100%);
            border-width: 1px;
            border-color: is-processing ? rgba(56, 189, 248, 0.4) : rgba(255, 255, 255, 0.12);
            opacity: reveal-main > 0.5 ? 1.0 : reveal-main * 2.0;
            drop-shadow-blur: 26px;
            drop-shadow-color: rgba(0, 0, 0, 0.55);

            // Fixed-width inner face, centre-anchored so the unfold
            // reads like the card flipping open.
            Rectangle {
                x: (parent.width - 340px) / 2;
                y: 0;
                width: 340px;
                height: 152px;

                // Holographic sheen sweeping across the card
                Rectangle {
                    x: sheen-x * 1px;
                    y: -20px;
                    width: 64px;
                    height: 192px;
                    background: @linear-gradient(90deg, rgba(255, 255, 255, 0) 0%, rgba(255, 255, 255, 0.06) 50%, rgba(255, 255, 255, 0) 100%);
                }

                // Gold chip
                Rectangle {
                    x: 20px; y: 18px; width: 38px; height: 28px;
                    border-radius: 6px;
                    background: @linear-gradient(160deg, #f6d27a 0%, #d9a946 55%, #b9842b 100%);

                    Rectangle { x: 0; y: 9px; width: 38px; height: 1.5px; background: rgba(70, 45, 0, 0.45); }
                    Rectangle { x: 0; y: 18px; width: 38px; height: 1.5px; background: rgba(70, 45, 0, 0.45); }
                    Rectangle { x: 18px; y: 0; width: 1.5px; height: 28px; background: rgba(70, 45, 0, 0.45); }
                }

                Text {
                    x: 68px; y: 16px;
                    text: "VOXCTRL";
                    color: #f3f4f6;
                    font-size: 14px;
                    font-weight: 850;
                    font-family: "Outfit";
                    letter-spacing: 1px;
                }
                Text {
                    x: 68px; y: 34px;
                    text: "VOICE CARD";
                    color: rgba(255, 255, 255, 0.38);
                    font-size: 7.5px;
                    font-weight: 800;
                    font-family: "Outfit";
                    letter-spacing: 3px;
                }

                // Status stamp (top right)
                Rectangle {
                    x: 252px; y: 18px; width: 68px; height: 20px;
                    border-radius: 5px;
                    border-width: 1px;
                    border-color: is-processing ? rgba(56, 189, 248, 0.5) : (!is-audio-ready ? rgba(245, 158, 11, 0.5) : rgba(244, 63, 94, 0.5));
                    background: is-processing ? rgba(56, 189, 248, 0.08) : (!is-audio-ready ? rgba(245, 158, 11, 0.08) : rgba(244, 63, 94, 0.08));

                    HorizontalLayout {
                        alignment: center;
                        spacing: 5px;

                        Rectangle {
                            width: 6px; height: 6px; border-radius: 3px;
                            y: (parent.height - self.height) / 2;
                            background: is-processing ? #38bdf8 : (!is-audio-ready ? #f59e0b : #f43f5e);
                            opacity: 0.35 + 0.65 * blink;
                        }
                        Text {
                            text: is-processing ? "PROC" : (!is-audio-ready ? "INIT" : "REC");
                            color: is-processing ? #38bdf8 : (!is-audio-ready ? #f59e0b : #f43f5e);
                            font-size: 8.5px;
                            font-weight: 850;
                            font-family: "Outfit";
                            letter-spacing: 1.5px;
                            vertical-alignment: center;
                        }
                    }
                }

                // VU-meter LED dot matrix (green→amber→red, bottom-up)
                HorizontalLayout {
                    x: 22px;
                    y: 56px;
                    width: 296px;
                    height: 51px;
                    spacing: 4px;

                    for col-level[ci] in led-cols : VerticalLayout {
                        spacing: 3px;

                        for r in [0, 1, 2, 3, 4, 5] : Rectangle {
                            width: 11px;
                            height: 6px;
                            border-radius: 2px;
                            background: is-processing ? #22d3ee : (r == 0 ? #f43f5e : (r <= 2 ? #fbbf24 : #34d399));
                            opacity: (6.0 - r) <= col-level * 6.0 ? 0.95 : 0.10;
                        }
                    }
                }

                // Embossed target field (bottom left)
                Text {
                    x: 22px; y: 118px;
                    text: "TARGET";
                    color: rgba(255, 255, 255, 0.30);
                    font-size: 7px;
                    font-weight: 800;
                    font-family: "Outfit";
                    letter-spacing: 2.5px;
                }
                Text {
                    x: 22px; y: 128px;
                    text: is-processing ? "Reading the card…" : target-label;
                    color: #e5e7eb;
                    font-size: 11.5px;
                    font-weight: 750;
                    font-family: "Outfit";
                    letter-spacing: 0.5px;
                    overflow: elide;
                    max-width: 200px;
                }

                // Card number flourish (bottom right)
                Text {
                    x: 236px; y: 128px;
                    text: "•••• VOX";
                    color: rgba(255, 255, 255, 0.22);
                    font-size: 11px;
                    font-weight: 700;
                    font-family: "Outfit";
                    letter-spacing: 2px;
                }
            }
        }

        // ─────────────────────────────────────────────────────────────
        // Speaking pill — slides up from the bottom while the system
        // responds, with a live mini-equalizer.
        // ─────────────────────────────────────────────────────────────
        if (reveal-pill > 0.004) : Rectangle {
            x: (560px - self.width) / 2;
            y: 134px + (1.0 - reveal-pill) * 20px;
            width: 280px;
            height: 46px;
            border-radius: 23px;
            background: rgba(4, 47, 36, 0.94);
            border-width: 1.2px;
            border-color: rgba(16, 185, 129, 0.55);
            opacity: Math.min(1.0, reveal-pill);
            drop-shadow-blur: 18px;
            drop-shadow-color: rgba(16, 185, 129, 0.25);

            HorizontalLayout {
                padding-left: 18px;
                padding-right: 18px;
                spacing: 10px;

                // Mini equalizer
                Rectangle {
                    width: 26px;

                    for h[i] in pill-bars : Rectangle {
                        x: i * 5.5px;
                        y: (parent.height - self.height) / 2;
                        width: 3px;
                        height: h * 1px;
                        border-radius: 1.5px;
                        background: #34d399;
                    }
                }

                VerticalLayout {
                    alignment: center;
                    spacing: 1px;

                    Text {
                        text: "SYSTEM RESPONDING";
                        color: #a7f3d0;
                        font-size: 10px;
                        font-weight: 850;
                        font-family: "Outfit";
                        letter-spacing: 1.5px;
                    }
                    Text {
                        text: "▸ " + target-label;
                        color: rgba(167, 243, 208, 0.5);
                        font-size: 8.5px;
                        font-weight: 600;
                        font-family: "Outfit";
                        overflow: elide;
                        max-width: 180px;
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

/// Critically-damped-ish spring used for load/unload (reveal) animations.
/// Slightly underdamped so overlays land with a subtle overshoot.
fn spring(x: &mut f32, v: &mut f32, target: f32, dt: f32) {
    let omega = 16.0_f32;
    let zeta = 0.78_f32;
    let a = omega * omega * (target - *x) - 2.0 * zeta * omega * *v;
    *v += a * dt;
    *x += *v * dt;
    if (*x - target).abs() < 0.001 && v.abs() < 0.02 {
        *x = target;
        *v = 0.0;
    }
}

/// Build a filled sine-wave path (for the ocean layers).
fn ocean_wave_path(width: f32, height: f32, amp: f32, freq: f32, phase: f32, y_off: f32) -> String {
    let mut d = String::with_capacity(1024);
    let _ = write!(d, "M 0 {height} L 0 {y_off:.1}");
    let mut x = 0.0_f32;
    while x <= width {
        let y = y_off + (x * freq + phase).sin() * amp;
        let _ = write!(d, " L {x:.0} {y:.1}");
        x += 8.0;
    }
    let _ = write!(d, " L {width} {height} Z");
    d
}

fn main() {
    let ui = OverlayWindow::new().unwrap();

    // Start with the window hidden. We show()/hide() dynamically.

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

    // ── Animation state owned by the render tick ────────────────────
    let mut reveal_main = 0.0_f32;
    let mut vel_main = 0.0_f32;
    let mut reveal_pill = 0.0_f32;
    let mut vel_pill = 0.0_f32;
    let mut phase = 0.0_f32;
    let mut cur_level = 0.0_f32;
    let mut osc_hist: VecDeque<f32> = VecDeque::from(vec![0.0_f32; 126]);
    let mut led_cols = [0.0_f32; 20];
    let mut lcg_state = 12345_u32;
    let mut shown = false;

    const DT: f32 = 0.016;

    timer.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(16), move || {
        let ui = match ui_weak.upgrade() {
            Some(ui) => ui,
            None => return,
        };

        let (rec, proc, speak, ready, raw_level, target, tx, ty, style) = {
            if let Ok(s) = state_clone2.lock() {
                (s.recording, s.processing, s.speaking, s.audio_ready, s.audio_level, s.active_target_label.clone(), s.x, s.y, s.overlay_style.clone())
            } else {
                return;
            }
        };

        // ── Animation clocks ─────────────────────────────────────────
        phase += DT;
        if phase > 100_000.0 {
            phase = 0.0;
        }
        let active_main = rec || proc;
        let active_pill = speak;
        spring(&mut reveal_main, &mut vel_main, if active_main { 1.0 } else { 0.0 }, DT);
        spring(&mut reveal_pill, &mut vel_pill, if active_pill { 1.0 } else { 0.0 }, DT);
        reveal_main = reveal_main.max(0.0);
        reveal_pill = reveal_pill.max(0.0);

        // Smoothed, normalized audio level (fast attack, fast release)
        let tgt_level = (raw_level * 100.0).clamp(0.0, 1.0);
        cur_level += (tgt_level - cur_level) * 0.35;

        let mut rand = || {
            lcg_state = lcg_state.wrapping_mul(1103515245).wrapping_add(12345);
            (lcg_state >> 8) as f32 / ((u32::MAX >> 8) as f32)
        };

        // ── Window visibility (kept alive through the unload anim) ──
        let visible_needed = active_main || active_pill || reveal_main > 0.004 || reveal_pill > 0.004;
        if visible_needed && !shown {
            if let Err(e) = ui.show() {
                eprintln!("[overlay] Failed to show window: {:?}", e);
            }
            shown = true;
        } else if !visible_needed && shown {
            if let Err(e) = ui.hide() {
                eprintln!("[overlay] Failed to hide window: {:?}", e);
            }
            shown = false;
        }

        if !shown {
            return;
        }
        if tx > -10000 {
            ui.window().set_position(slint::PhysicalPosition::new(tx, ty));
        }

        // ── Shared status properties ────────────────────────────────
        ui.set_is_recording(rec);
        ui.set_is_processing(proc);
        ui.set_is_speaking(speak);
        ui.set_is_audio_ready(ready);
        ui.set_target_label(target.into());
        ui.set_overlay_style(style.clone().into());
        ui.set_reveal_main(reveal_main.min(1.12));
        ui.set_reveal_pill(reveal_pill.min(1.12));
        ui.set_level(cur_level);
        ui.set_blink(0.5 + 0.5 * (phase * 5.5).sin());

        // ── Per-style visualizer data ────────────────────────────────
        match style.as_str() {
            // Oscilloscope: scroll a live trace through a ring buffer.
            "waveform" => {
                let sample = if proc {
                    0.55 * (phase * 9.0).sin() * (0.6 + 0.4 * (phase * 1.7).sin())
                } else if rec && !ready {
                    0.05 * (rand() * 2.0 - 1.0)
                } else if rec {
                    let noise = rand() * 2.0 - 1.0;
                    (cur_level * 1.5).min(1.0) * (0.45 * (phase * 24.0).sin() + 0.55 * noise)
                } else {
                    0.0
                };
                osc_hist.pop_front();
                osc_hist.push_back(sample);

                let mut d = String::with_capacity(2048);
                for (i, v) in osc_hist.iter().enumerate() {
                    let x = i as f32 * 4.0;
                    let y = (39.0 - v * 35.0).clamp(2.0, 76.0);
                    if i == 0 {
                        let _ = write!(d, "M {x:.0} {y:.1}");
                    } else {
                        let _ = write!(d, " L {x:.0} {y:.1}");
                    }
                }
                ui.set_osc_path(d.into());
            }

            // Radar: sweep arm, trail, expanding pulse rings and blips.
            "pulse" => {
                let cx = 62.0_f32;
                let cy = 62.0_f32;
                let r = 56.0_f32;
                let a = phase * 4.2;
                let main = format!(
                    "M {cx} {cy} L {:.1} {:.1}",
                    cx + a.cos() * r,
                    cy + a.sin() * r
                );
                let mut trail = String::with_capacity(256);
                for k in 1..=4 {
                    let ta = a - 0.13 * k as f32;
                    let _ = write!(
                        trail,
                        "M {cx} {cy} L {:.1} {:.1} ",
                        cx + ta.cos() * (r - 2.0 * k as f32),
                        cy + ta.sin() * (r - 2.0 * k as f32)
                    );
                }
                ui.set_sweep_path(main.into());
                ui.set_sweep_trail_path(trail.into());

                // Pulse rings emitted from the core; louder voice = brighter rings.
                let t1 = (phase * 0.75).fract();
                let t2 = (phase * 0.75 + 0.5).fract();
                ui.set_ring1_s(22.0 + t1 * 96.0);
                ui.set_ring1_o((1.0 - t1) * (0.2 + cur_level * 0.65));
                ui.set_ring2_s(22.0 + t2 * 96.0);
                ui.set_ring2_o((1.0 - t2) * (0.2 + cur_level * 0.65));

                // Blips flash as the sweep passes their bearing.
                let two_pi = std::f32::consts::TAU;
                let blip = |bearing: f32| -> f32 {
                    let d = (a - bearing).rem_euclid(two_pi);
                    (1.0 - d / 2.2).clamp(0.0, 1.0)
                };
                ui.set_blip1_o(blip(-0.35));
                ui.set_blip2_o(blip(2.45));
            }

            // Ocean: three layered waves whose tide rises with the voice.
            // reveal-main scales the fill so water drains out on unload.
            "blue_wave" => {
                let fill = reveal_main.min(1.0);
                let lift = cur_level;
                let proc_surge = if proc { 4.0 + 2.5 * (phase * 1.6).sin() } else { 0.0 };

                let y1 = 64.0 - lift * 20.0 - proc_surge;
                let y2 = 56.0 - lift * 24.0 - proc_surge * 1.2;
                let y3 = 47.0 - lift * 28.0 - proc_surge * 1.4;
                let drain = |y: f32| 90.0 - (90.0 - y) * fill;

                let a1 = (if proc { 5.0 } else { 2.5 }) + lift * 14.0;
                let a2 = (if proc { 6.0 } else { 2.0 }) + lift * 18.0;
                let a3 = (if proc { 7.0 } else { 1.5 }) + lift * 22.0;

                let y3_final = drain(y3);
                ui.set_ocean_path1(ocean_wave_path(380.0, 90.0, a1, 0.014, phase * 2.1, drain(y1)).into());
                ui.set_ocean_path2(ocean_wave_path(380.0, 90.0, a2, 0.022, -phase * 3.0, drain(y2)).into());
                ui.set_ocean_path3(ocean_wave_path(380.0, 90.0, a3, 0.018, phase * 3.9, y3_final).into());

                // Buoy bobs on the front wave's surface (panel coords).
                ui.set_buoy_y(38.0 + y3_final - 30.0 + (phase * 2.2).sin() * 2.5);

                // Bubbles rise faster while there is voice energy.
                let bubbles = std::rc::Rc::new(slint::VecModel::default());
                for i in 0..3 {
                    let speed = 14.0 + cur_level * 26.0;
                    let yy = 84.0 - (phase * speed + i as f32 * 31.0).rem_euclid(78.0);
                    let o = ((yy / 84.0) * 0.55).clamp(0.08, 0.6) * fill;
                    bubbles.push(BubbleData { y: 38.0 + yy, o });
                }
                ui.set_bubbles(bubbles.into());
            }

            // Voice card: VU dot-matrix column levels with peak decay.
            "voice_card" => {
                let n = led_cols.len();
                for i in 0..n {
                    let target = if proc {
                        0.25 + 0.75 * ((phase * 4.0 - i as f32 * 0.45).sin()).max(0.0)
                    } else if rec && !ready {
                        0.08 + 0.06 * rand()
                    } else {
                        let mid = (n as f32 - 1.0) / 2.0;
                        let env = (-((i as f32 - mid).powi(2)) / 42.0).exp();
                        (cur_level * (0.55 + 0.65 * rand()) * env * 1.9).min(1.0)
                    };
                    if target > led_cols[i] {
                        led_cols[i] = target;
                    } else {
                        led_cols[i] *= 0.86;
                    }
                }
                let cols = std::rc::Rc::new(slint::VecModel::default());
                for c in led_cols.iter() {
                    cols.push(*c);
                }
                ui.set_led_cols(cols.into());
            }

            _ => {}
        }

        // Holo sheen keeps drifting across the voice card.
        if style == "voice_card" {
            ui.set_sheen_x((phase * 70.0).rem_euclid(480.0) - 70.0);
        }

        // Speaking pill mini-equalizer.
        if reveal_pill > 0.004 {
            let bars = std::rc::Rc::new(slint::VecModel::default());
            for i in 0..5 {
                bars.push(8.0_f32 + 13.0 * (0.5 + 0.5 * (phase * 8.0 + i as f32 * 1.05).sin()));
            }
            ui.set_pill_bars(bars.into());
        }
    });

    slint::run_event_loop_until_quit().unwrap();
}
