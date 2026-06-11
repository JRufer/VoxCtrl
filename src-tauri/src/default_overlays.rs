use std::path::Path;

// ─────────────────────────────────────────────────────────────────────
// Gradient Wave — "Aurora": flowing multi-color gradient ribbons over a
// starlit night panel. The ribbons' amplitude tracks the microphone
// level; the whole sky sweeps in on load and drifts away on unload.
// ─────────────────────────────────────────────────────────────────────

pub const GRADIENT_WAVE_HTML: &str = r##"<div class="aurora-card">
  <span class="star s1"></span>
  <span class="star s2"></span>
  <span class="star s3"></span>
  <span class="star s4"></span>
  <span class="star s5"></span>
  <span class="star s6"></span>

  <div class="aurora-header">
    <span class="aurora-led"></span>
    <span class="aurora-title">GRADIENT WAVE</span>
    <span class="aurora-sub" id="auroraState">aurora — listening</span>
    <span class="aurora-target">
      <span class="aurora-target-arrow">▸</span>
      <span class="aurora-target-label" id="auroraTarget">{{target}}</span>
    </span>
  </div>

  <svg class="aurora-svg" width="430" height="78" viewBox="0 0 430 78" xmlns="http://www.w3.org/2000/svg">
    <defs>
      <linearGradient id="auroraGrad1" x1="0%" y1="0%" x2="100%" y2="0%">
        <stop offset="0%" stop-color="#a855f7"/>
        <stop offset="50%" stop-color="#ec4899"/>
        <stop offset="100%" stop-color="#22d3ee"/>
      </linearGradient>
      <linearGradient id="auroraGrad2" x1="0%" y1="0%" x2="100%" y2="0%">
        <stop offset="0%" stop-color="#22d3ee"/>
        <stop offset="55%" stop-color="#818cf8"/>
        <stop offset="100%" stop-color="#e879f9"/>
      </linearGradient>
      <linearGradient id="auroraGrad3" x1="0%" y1="0%" x2="100%" y2="0%">
        <stop offset="0%" stop-color="#f472b6"/>
        <stop offset="50%" stop-color="#a78bfa"/>
        <stop offset="100%" stop-color="#34d399"/>
      </linearGradient>
    </defs>
    <path id="auroraRibbon1" fill="none" stroke="url(#auroraGrad1)" stroke-width="3" stroke-linecap="round" class="ribbon ribbon-1"/>
    <path id="auroraRibbon2" fill="none" stroke="url(#auroraGrad2)" stroke-width="2" stroke-linecap="round" class="ribbon ribbon-2"/>
    <path id="auroraRibbon3" fill="none" stroke="url(#auroraGrad3)" stroke-width="1.5" stroke-linecap="round" class="ribbon ribbon-3"/>
  </svg>

  <script>
    (function() {
      let targetVolume = 0;
      let currentVolume = 0;
      let processing = false;
      let ready = true;
      let time = 0;
      let animationFrameId = null;

      function onLevel(e) {
        targetVolume = Math.min(1.0, e.detail * 100.0);
      }

      function onStatus(e) {
        processing = !!e.detail.processing;
        ready = e.detail.audio_ready !== false;

        var t = document.getElementById("auroraTarget");
        if (t && e.detail.active_target_label) t.textContent = e.detail.active_target_label;

        var s = document.getElementById("auroraState");
        if (s) {
          s.textContent = processing ? "solar storm — processing"
            : (!ready ? "gathering light…" : "aurora — listening");
        }

        var card = document.querySelector(".aurora-card");
        if (card) card.classList.toggle("processing", processing);
      }

      window.addEventListener("voxctrl-audio-level", onLevel);
      window.addEventListener("voxctrl-status", onStatus);

      var W = 430, MID = 42;

      // A ribbon is an open sine stroke whose amplitude fades to zero at
      // both edges, so it reads as a band of light rather than water.
      function ribbon(amp, freq, ph, drift) {
        var d = "";
        for (var x = 0; x <= W; x += 6) {
          var env = Math.sin((x / W) * Math.PI);
          var y = MID
            + Math.sin(x * freq + ph) * amp * env
            + Math.sin(x * 0.012 + ph * 0.4) * drift * env;
          d += (x === 0 ? "M " : " L ") + x + " " + y.toFixed(1);
        }
        return d;
      }

      var p1 = document.getElementById("auroraRibbon1");
      var p2 = document.getElementById("auroraRibbon2");
      var p3 = document.getElementById("auroraRibbon3");

      function draw() {
        time += 0.016;
        currentVolume += (targetVolume - currentVolume) * 0.35;
        targetVolume *= 0.86;

        var speed = processing ? 2.2 : 1.0;
        var base = processing ? 10 : 4;
        var lift = currentVolume * 26;

        if (p1) p1.setAttribute("d", ribbon(base + lift, 0.020, time * 2.0 * speed, 6));
        if (p2) p2.setAttribute("d", ribbon(base * 0.8 + lift * 0.75, 0.027, -time * 2.6 * speed, 8));
        if (p3) p3.setAttribute("d", ribbon(base * 0.6 + lift * 0.5, 0.034, time * 3.4 * speed, 10));

        animationFrameId = requestAnimationFrame(draw);
      }
      draw();

      window.addEventListener("voxctrl-cleanup", function() {
        window.removeEventListener("voxctrl-audio-level", onLevel);
        window.removeEventListener("voxctrl-status", onStatus);
        if (animationFrameId) cancelAnimationFrame(animationFrameId);
      }, { once: true });
    })();
  </script>
</div>
"##;

pub const GRADIENT_WAVE_CSS: &str = r##".aurora-card {
  position: relative;
  width: 470px;
  height: 130px;
  box-sizing: border-box;
  padding: 14px 20px 10px;
  border-radius: 20px;
  overflow: hidden;
  background:
    radial-gradient(120% 90% at 50% -20%, rgba(88, 28, 135, 0.35) 0%, rgba(8, 8, 16, 0) 60%),
    linear-gradient(180deg, #0b0a14 0%, #060510 100%);
  border: 1px solid rgba(168, 85, 247, 0.28);
  box-shadow: 0 16px 44px rgba(0, 0, 0, 0.6), 0 0 26px rgba(168, 85, 247, 0.18);
  font-family: 'Outfit', 'Inter', sans-serif;
  pointer-events: none;
  user-select: none;

  /* Load/unload: the whole sky rises in and drifts away */
  opacity: 0;
  transform: translateY(16px) scale(0.97);
  transition:
    transform 0.38s cubic-bezier(0.175, 0.885, 0.32, 1.2),
    opacity 0.3s ease;
}

.custom-overlay-content.active .aurora-card {
  opacity: 1;
  transform: translateY(0) scale(1);
}

/* Twinkling stars */
.aurora-card .star {
  position: absolute;
  width: 2px;
  height: 2px;
  border-radius: 50%;
  background: rgba(224, 231, 255, 0.9);
  animation: aurora-twinkle 2.6s ease-in-out infinite;
}
.aurora-card .s1 { left: 60px;  top: 38px; }
.aurora-card .s2 { left: 150px; top: 30px; animation-delay: 0.5s; }
.aurora-card .s3 { left: 250px; top: 42px; animation-delay: 1.1s; }
.aurora-card .s4 { left: 330px; top: 28px; animation-delay: 1.6s; }
.aurora-card .s5 { left: 400px; top: 40px; animation-delay: 0.8s; }
.aurora-card .s6 { left: 210px; top: 55px; animation-delay: 2.0s; }

@keyframes aurora-twinkle {
  0%, 100% { opacity: 0.15; }
  50% { opacity: 0.9; }
}

.aurora-header {
  position: relative;
  display: flex;
  align-items: center;
  gap: 8px;
  height: 18px;
  z-index: 2;
}

.aurora-led {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: #e879f9;
  box-shadow: 0 0 8px #e879f9;
  animation: aurora-twinkle 1.2s ease-in-out infinite;
  flex-shrink: 0;
}

.aurora-card.processing .aurora-led {
  background: #38bdf8;
  box-shadow: 0 0 8px #38bdf8;
}

.aurora-title {
  font-size: 10px;
  font-weight: 800;
  letter-spacing: 0.2em;
  background: linear-gradient(90deg, #e879f9, #a78bfa, #22d3ee);
  -webkit-background-clip: text;
  background-clip: text;
  color: transparent;
}

.aurora-sub {
  font-size: 8.5px;
  font-weight: 500;
  letter-spacing: 0.08em;
  color: rgba(216, 180, 254, 0.45);
}

.aurora-target {
  margin-left: auto;
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 2.5px 10px;
  border-radius: 99px;
  background: rgba(168, 85, 247, 0.08);
  border: 1px solid rgba(168, 85, 247, 0.4);
}

.aurora-target-arrow {
  color: #e879f9;
  font-size: 9px;
  font-weight: 800;
}

.aurora-target-label {
  color: #f5f3ff;
  font-size: 9px;
  font-weight: 700;
  letter-spacing: 0.05em;
  max-width: 140px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* Ribbons sweep in from the left, staggered, and sweep back on unload */
.aurora-svg {
  display: block;
  margin-top: 6px;
}

.aurora-svg .ribbon {
  opacity: 0;
  transform: translateX(-30px);
  transition:
    transform 0.5s cubic-bezier(0.22, 1, 0.36, 1),
    opacity 0.35s ease;
}
.aurora-svg .ribbon-2 { transition-delay: 0.07s; }
.aurora-svg .ribbon-3 { transition-delay: 0.14s; }

.custom-overlay-content.active .aurora-card .ribbon {
  opacity: 1;
  transform: translateX(0);
}

.aurora-svg .ribbon-1 { filter: drop-shadow(0 0 6px rgba(236, 72, 153, 0.55)); }
.aurora-svg .ribbon-2 { filter: drop-shadow(0 0 5px rgba(129, 140, 248, 0.5)); }
.aurora-svg .ribbon-3 { filter: drop-shadow(0 0 4px rgba(52, 211, 153, 0.45)); }
"##;

// ─────────────────────────────────────────────────────────────────────
// Rainbow — an actual rainbow: seven ROYGBIV arc bands that draw
// themselves over a dark cloud capsule holding a scrolling spectrum
// analyzer. The target label lives at the rainbow's end (the pot).
// ─────────────────────────────────────────────────────────────────────

pub const RAINBOW_HTML: &str = r##"<div class="rainbow-card">
  <svg class="rainbow-arc" width="300" height="120" viewBox="0 0 300 120" xmlns="http://www.w3.org/2000/svg" id="rainbowArc">
    <path class="arc arc-1" d="M 38 118 A 112 112 0 0 1 262 118" stroke="#ef4444" pathLength="100"/>
    <path class="arc arc-2" d="M 45 118 A 105 105 0 0 1 255 118" stroke="#f97316" pathLength="100"/>
    <path class="arc arc-3" d="M 52 118 A 98 98 0 0 1 248 118" stroke="#fbbf24" pathLength="100"/>
    <path class="arc arc-4" d="M 59 118 A 91 91 0 0 1 241 118" stroke="#22c55e" pathLength="100"/>
    <path class="arc arc-5" d="M 66 118 A 84 84 0 0 1 234 118" stroke="#38bdf8" pathLength="100"/>
    <path class="arc arc-6" d="M 73 118 A 77 77 0 0 1 227 118" stroke="#6366f1" pathLength="100"/>
    <path class="arc arc-7" d="M 80 118 A 70 70 0 0 1 220 118" stroke="#a855f7" pathLength="100"/>
  </svg>

  <div class="black-capsule">
    <div class="glass-glare"></div>
    <div class="spectrum-analyzer" id="spectrumAnalyzer">
      <div class="bar bar-1"></div>
      <div class="bar bar-2"></div>
      <div class="bar bar-3"></div>
      <div class="bar bar-4"></div>
      <div class="bar bar-5"></div>
      <div class="bar bar-6"></div>
      <div class="bar bar-7"></div>
      <div class="bar bar-8"></div>
      <div class="bar bar-9"></div>
      <div class="bar bar-10"></div>
      <div class="bar bar-11"></div>
      <div class="bar bar-12"></div>
      <div class="bar bar-13"></div>
      <div class="bar bar-14"></div>
      <div class="bar bar-15"></div>
    </div>
  </div>

  <div class="pot-bar">
    <span class="pot-dot" id="rainbowDot"></span>
    <span class="keybind-title" id="rainbowTarget">{{target}}</span>
  </div>

  <script>
    (function() {
      let isActive = false;
      let processing = false;
      let ready = true;
      let audioLevel = 0;
      let smoothLevel = 0;
      let animationFrameId = null;

      function handleStatus(e) {
        const { recording, speaking } = e.detail;
        processing = !!e.detail.processing;
        ready = e.detail.audio_ready !== false;
        isActive = recording || processing || speaking;

        const t = document.getElementById("rainbowTarget");
        if (t) {
          t.textContent = processing ? "Painting the response…"
            : (!ready ? "Waiting for sunshine…"
              : (e.detail.active_target_label || t.textContent));
        }

        const dot = document.getElementById("rainbowDot");
        if (dot) {
          dot.style.background = processing ? "#38bdf8" : (!ready ? "#f59e0b" : "#34d399");
          dot.style.boxShadow = "0 0 8px " + (processing ? "#38bdf8" : (!ready ? "#f59e0b" : "#34d399"));
        }
      }

      function handleAudioLevel(e) {
        // Calibrated like the built-in styles: raw RMS scaled x100 into 0..1
        audioLevel = Math.min(1.0, e.detail * 100.0);
      }

      window.addEventListener("voxctrl-status", handleStatus);
      window.addEventListener("voxctrl-audio-level", handleAudioLevel);

      const bars = document.querySelectorAll(".rainbow-card .bar");
      const arcSvg = document.getElementById("rainbowArc");
      const barCount = 15;

      // Shift register / propagation queue for history (0 is left, 14 is right)
      const history = Array(barCount).fill(0.0);
      const scrollSpeed = 0.35; // Controls wave propagation speed to the left
      let time = 0;

      function draw() {
        time += 0.05; // Increment time for breathing/vibration
        const currentInput = isActive ? audioLevel : 0.0;
        smoothLevel += (currentInput - smoothLevel) * 0.2;

        // The rightmost bar (bar 15 / index 14) receives the new input
        history[barCount - 1] += (currentInput - history[barCount - 1]) * 0.45;

        // Propagate values from right to left smoothly (ascending order uses previous frame values)
        for (let i = 0; i < barCount - 1; i++) {
          history[i] += (history[i + 1] - history[i]) * scrollSpeed;
        }

        // Render the bars
        for (let i = 0; i < barCount; i++) {
          // Taper factor: 0.0 on leftmost, 1.0 on rightmost.
          // This makes waves shrink/fade off to nothing as they travel left.
          const taper = i / (barCount - 1);

          // Add a tiny slow idle breathing pulse when quiet to force WebKitGTK compositing updates
          const breathe = 0.03 * Math.sin(time + i * 0.4);
          const val = (history[i] + breathe * (1.0 - history[i])) * taper;
          const norm = Math.max(0.0, Math.min(1.0, val));

          // Height in pixels (min 4px, max 36px)
          const heightPx = 4 + norm * 36;

          if (bars[i]) {
            bars[i].style.height = heightPx + "px";
            // Taper the opacity too, so it physically fades to nothing
            bars[i].style.opacity = 0.25 + (taper * norm * 0.75);
          }
        }

        // The rainbow glows brighter the louder you speak
        if (arcSvg) {
          arcSvg.style.opacity = (0.72 + smoothLevel * 0.28).toFixed(3);
        }

        animationFrameId = requestAnimationFrame(draw);
      }

      draw();

      window.addEventListener("voxctrl-cleanup", function() {
        window.removeEventListener("voxctrl-status", handleStatus);
        window.removeEventListener("voxctrl-audio-level", handleAudioLevel);
        if (animationFrameId) {
          cancelAnimationFrame(animationFrameId);
        }
      }, { once: true });
    })();
  </script>
</div>
"##;

pub const RAINBOW_CSS: &str = r##".rainbow-card {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: flex-end;
  width: 480px;
  height: 168px;
  box-sizing: border-box;
  pointer-events: none;
  user-select: none;
  position: relative;
}

/* ── The rainbow: seven bands that draw themselves in on load and
      un-draw on unload, staggered outer-to-inner ───────────────── */
.rainbow-arc {
  display: block;
  margin-bottom: -46px; /* the arc rises from behind the cloud capsule */
  z-index: 1;
}

.rainbow-arc .arc {
  fill: none;
  stroke-width: 6;
  stroke-linecap: round;
  stroke-dasharray: 100;
  stroke-dashoffset: 100;
  opacity: 0;
  transition:
    stroke-dashoffset 0.6s cubic-bezier(0.22, 1, 0.36, 1),
    opacity 0.25s ease;
}
.rainbow-arc .arc-2 { transition-delay: 0.05s; }
.rainbow-arc .arc-3 { transition-delay: 0.10s; }
.rainbow-arc .arc-4 { transition-delay: 0.15s; }
.rainbow-arc .arc-5 { transition-delay: 0.20s; }
.rainbow-arc .arc-6 { transition-delay: 0.25s; }
.rainbow-arc .arc-7 { transition-delay: 0.30s; }

.custom-overlay-content.active .rainbow-card .rainbow-arc .arc {
  stroke-dashoffset: 0;
  opacity: 1;
}

/* ── Cloud capsule with the scrolling spectrum analyzer ────────── */
.rainbow-card .black-capsule {
  position: relative;
  width: 440px;
  height: 64px;
  background: linear-gradient(180deg, #1b1b1f 0%, #0a0a0c 100%);
  border: 1.5px solid #000000;
  border-radius: 9999px;
  display: flex;
  align-items: center;
  justify-content: center;
  box-shadow: 0 16px 40px rgba(0, 0, 0, 0.75),
              inset 0 1px 0 rgba(255, 255, 255, 0.16),
              inset 0 0 0 1px rgba(255, 255, 255, 0.04);
  z-index: 10;
  overflow: hidden;

  /* Initial hidden state */
  transform: scale(0);
  opacity: 0;
  transition: transform 0.45s cubic-bezier(0.34, 1.56, 0.64, 1.1), opacity 0.35s ease;
}

/* Transitions to active state via .custom-overlay-content.active */
.custom-overlay-content.active .rainbow-card .black-capsule {
  transform: scale(1);
  opacity: 1;
}

.glass-glare {
  position: absolute;
  top: 1.5px;
  left: 16px;
  right: 16px;
  height: 50%;
  background: linear-gradient(180deg, rgba(255, 255, 255, 0.16) 0%, rgba(255, 255, 255, 0) 100%);
  border-radius: 9999px 9999px 0 0;
  pointer-events: none;
  z-index: 2;
}

.spectrum-analyzer {
  display: flex;
  flex-direction: row;
  align-items: flex-end;
  justify-content: space-between;
  width: 360px;
  height: 44px;
  z-index: 1;
  padding-bottom: 8px;
  box-sizing: border-box;
}

.bar {
  width: 10px;
  height: 5px;
  border-radius: 9999px;
  /* No CSS height transition so JS requestAnimationFrame updates are immediate and stutter-free */
}

/* Horizontal ROYGBIV spectrum with matching glows */
.bar-1  { background: #ef4444; box-shadow: 0 0 12px rgba(239, 68, 68, 0.65); }
.bar-2  { background: #f8633c; box-shadow: 0 0 12px rgba(248, 99, 60, 0.65); }
.bar-3  { background: #f97316; box-shadow: 0 0 12px rgba(249, 115, 22, 0.65); }
.bar-4  { background: #fb9a1f; box-shadow: 0 0 12px rgba(251, 154, 31, 0.65); }
.bar-5  { background: #fbbf24; box-shadow: 0 0 12px rgba(251, 191, 36, 0.65); }
.bar-6  { background: #a3d635; box-shadow: 0 0 12px rgba(163, 214, 53, 0.65); }
.bar-7  { background: #22c55e; box-shadow: 0 0 12px rgba(34, 197, 94, 0.65); }
.bar-8  { background: #2dd4bf; box-shadow: 0 0 16px rgba(45, 212, 191, 0.85); }
.bar-9  { background: #38bdf8; box-shadow: 0 0 12px rgba(56, 189, 248, 0.65); }
.bar-10 { background: #4f93f7; box-shadow: 0 0 12px rgba(79, 147, 247, 0.65); }
.bar-11 { background: #6366f1; box-shadow: 0 0 12px rgba(99, 102, 241, 0.65); }
.bar-12 { background: #7c5cf5; box-shadow: 0 0 12px rgba(124, 92, 245, 0.65); }
.bar-13 { background: #a855f7; box-shadow: 0 0 12px rgba(168, 85, 247, 0.65); }
.bar-14 { background: #c44df0; box-shadow: 0 0 12px rgba(196, 77, 240, 0.65); }
.bar-15 { background: #e879f9; box-shadow: 0 0 12px rgba(232, 121, 249, 0.65); }

/* ── The pot at the end of the rainbow: target indicator ───────── */
.pot-bar {
  margin-top: 9px;
  display: inline-flex;
  align-items: center;
  gap: 8px;
  background: rgba(29, 78, 216, 0.18);
  border: 1.2px solid rgba(59, 130, 246, 0.4);
  border-radius: 9999px;
  padding: 5px 22px;
  box-shadow: 0 4px 16px rgba(59, 130, 246, 0.28);
  justify-content: center;
  z-index: 5;

  /* Initial hidden state */
  opacity: 0;
  transform: translateY(12px);
  transition: opacity 0.35s ease 0.1s, transform 0.4s cubic-bezier(0.16, 1, 0.3, 1) 0.1s;
}

/* Transitions to active state via .custom-overlay-content.active */
.custom-overlay-content.active .rainbow-card .pot-bar {
  opacity: 1;
  transform: translateY(0);
}

.pot-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: #34d399;
  box-shadow: 0 0 8px #34d399;
  animation: pot-blink 1.15s ease-in-out infinite;
  flex-shrink: 0;
}

@keyframes pot-blink {
  0%, 100% { opacity: 0.4; }
  50% { opacity: 1; }
}

.keybind-title {
  font-family: 'Outfit', 'Inter', system-ui, sans-serif;
  font-size: 12.5px;
  font-weight: 650;
  color: #ffffff;
  letter-spacing: 0.01em;
  text-shadow: 0 1px 2px rgba(0, 0, 0, 0.3);
  max-width: 220px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
"##;

pub fn ensure_default_overlays(overlays_dir: &Path) {
    let gw_dir = overlays_dir.join("gradient-wave");
    if let Ok(_) = std::fs::create_dir_all(&gw_dir) {
        let _ = std::fs::write(gw_dir.join("index.html"), GRADIENT_WAVE_HTML);
        let _ = std::fs::write(gw_dir.join("style.css"), GRADIENT_WAVE_CSS);
    }

    let rb_dir = overlays_dir.join("rainbow");
    if let Ok(_) = std::fs::create_dir_all(&rb_dir) {
        let _ = std::fs::write(rb_dir.join("index.html"), RAINBOW_HTML);
        let _ = std::fs::write(rb_dir.join("style.css"), RAINBOW_CSS);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both templates must show the active routing target.
    #[test]
    fn templates_include_a_target_indicator() {
        for (name, html) in [("gradient-wave", GRADIENT_WAVE_HTML), ("rainbow", RAINBOW_HTML)] {
            assert!(
                html.contains("{{target}}"),
                "{name} template must include the target placeholder"
            );
        }
    }

    /// Both templates must define hidden base states and reveal them via the
    /// `.custom-overlay-content.active` class, which the overlay host toggles
    /// to drive load and unload animations.
    #[test]
    fn templates_have_load_and_unload_animations() {
        for (name, css) in [("gradient-wave", GRADIENT_WAVE_CSS), ("rainbow", RAINBOW_CSS)] {
            assert!(
                css.contains(".custom-overlay-content.active"),
                "{name} must animate via the active class"
            );
            assert!(
                css.contains("opacity: 0"),
                "{name} must define a hidden base state to animate from"
            );
            assert!(css.contains("transition"), "{name} must define transitions");
        }
    }

    /// Both templates must react to the live audio level stream.
    #[test]
    fn templates_have_audio_reactive_visualizers() {
        for (name, html) in [("gradient-wave", GRADIENT_WAVE_HTML), ("rainbow", RAINBOW_HTML)] {
            assert!(
                html.contains("voxctrl-audio-level"),
                "{name} must subscribe to audio levels"
            );
            assert!(
                html.contains("requestAnimationFrame"),
                "{name} must run an animation loop"
            );
        }
    }

    /// Both templates must reflect recording/processing/initializing states.
    #[test]
    fn templates_react_to_status_changes() {
        for (name, html) in [("gradient-wave", GRADIENT_WAVE_HTML), ("rainbow", RAINBOW_HTML)] {
            assert!(
                html.contains("voxctrl-status"),
                "{name} must subscribe to status updates"
            );
        }
    }

    /// Both templates must tear their listeners down when the host unmounts.
    #[test]
    fn templates_clean_up_after_themselves() {
        for (name, html) in [("gradient-wave", GRADIENT_WAVE_HTML), ("rainbow", RAINBOW_HTML)] {
            assert!(
                html.contains("voxctrl-cleanup"),
                "{name} must release listeners on cleanup"
            );
            assert!(
                html.contains("cancelAnimationFrame"),
                "{name} must cancel its animation loop on cleanup"
            );
        }
    }

    /// ensure_default_overlays must (re)write both bundled overlay folders so
    /// users always receive the current designs after an update.
    #[test]
    fn ensure_default_overlays_writes_both_templates() {
        let dir = std::env::temp_dir().join(format!(
            "voxctrl-overlay-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        ensure_default_overlays(&dir);

        for name in ["gradient-wave", "rainbow"] {
            let html = std::fs::read_to_string(dir.join(name).join("index.html")).unwrap();
            let css = std::fs::read_to_string(dir.join(name).join("style.css")).unwrap();
            assert!(!html.is_empty() && !css.is_empty(), "{name} files must not be empty");
        }

        // Stale designs must be overwritten on the next launch.
        std::fs::write(dir.join("rainbow").join("style.css"), "outdated").unwrap();
        ensure_default_overlays(&dir);
        let css = std::fs::read_to_string(dir.join("rainbow").join("style.css")).unwrap();
        assert_eq!(css, RAINBOW_CSS, "bundled overlays must be refreshed at launch");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
