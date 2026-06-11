use std::path::Path;

pub const GRADIENT_WAVE_HTML: &str = r##"<div class="custom-ocean-card">
  <div class="header">
    <span class="label">Voice Visualizer (Custom)</span>
    <span class="badge">{{target}}</span>
  </div>
  
  <div class="ocean">
    <svg width="288" height="55" viewBox="0 0 288 55" xmlns="http://www.w3.org/2000/svg">
      <defs>
        <!-- Deep Blue Ocean Gradient -->
        <linearGradient id="blueGrad1" x1="0%" y1="0%" x2="0%" y2="100%">
          <stop offset="0%" stop-color="#0284c7" stop-opacity="0.7"/>
          <stop offset="100%" stop-color="#075985" stop-opacity="0.1"/>
        </linearGradient>
        <!-- Cyan Aqua Gradient -->
        <linearGradient id="blueGrad2" x1="0%" y1="0%" x2="0%" y2="100%">
          <stop offset="0%" stop-color="#06b6d4" stop-opacity="0.8"/>
          <stop offset="100%" stop-color="#0e7490" stop-opacity="0.1"/>
        </linearGradient>
        <!-- Bright Ice Teal Gradient -->
        <linearGradient id="blueGrad3" x1="0%" y1="0%" x2="0%" y2="100%">
          <stop offset="0%" stop-color="#22d3ee" stop-opacity="0.95"/>
          <stop offset="100%" stop-color="#0891b2" stop-opacity="0.2"/>
        </linearGradient>
      </defs>
      <!-- Wave 1 (Deep Ocean Blue) -->
      <path id="customWave1" fill="url(#blueGrad1)" stroke="rgba(2, 132, 199, 0.25)" stroke-width="1"/>
      <!-- Wave 2 (Rich Cyan Aqua) -->
      <path id="customWave2" fill="url(#blueGrad2)" stroke="rgba(6, 182, 212, 0.3)" stroke-width="1"/>
      <!-- Wave 3 (Vibrant Ice Teal) -->
      <path id="customWave3" fill="url(#blueGrad3)" stroke="rgba(34, 211, 238, 0.5)" stroke-width="1.5"/>
    </svg>
  </div>

  <script>
    let time = 0;
    let targetVolume = 0;
    let currentVolume = 0;

    // Listen to dynamic voxctrl level events
    window.addEventListener("voxctrl-audio-level", (e) => {
      targetVolume = Math.min(1.0, e.detail * 100.0);
    });

    const width = 288;
    const height = 55;

    function getWavePath(amplitude, frequency, phase, yOffset) {
      let points = [];
      for (let x = 0; x <= width; x += 4) {
        const y = yOffset + Math.sin(x * frequency + phase) * amplitude;
        points.push(x + "," + y);
      }
      return "M 0," + height + " L 0," + yOffset + " " + points.map(p => "L " + p).join(' ') + " L " + width + "," + height + " Z";
    }

    function update() {
      time += 1;
      currentVolume += (targetVolume - currentVolume) * 0.38;
      targetVolume *= 0.84;

      // Compute dynamic wave properties based on current volume level
      const amp1 = 2.5 + currentVolume * 14.0;
      const yOff1 = 38 - currentVolume * 12.0;
      const path1 = getWavePath(amp1, 0.016, time * 0.035, yOff1);

      const amp2 = 2.0 + currentVolume * 18.0;
      const yOff2 = 34 - currentVolume * 14.0;
      const path2 = getWavePath(amp2, 0.024, -time * 0.05, yOff2);

      const amp3 = 1.5 + currentVolume * 22.0;
      const yOff3 = 28 - currentVolume * 16.0;
      const path3 = getWavePath(amp3, 0.020, time * 0.065, yOff3);

      const w1 = document.getElementById("customWave1");
      const w2 = document.getElementById("customWave2");
      const w3 = document.getElementById("customWave3");

      if (w1) w1.setAttribute("d", path1);
      if (w2) w2.setAttribute("d", path2);
      if (w3) w3.setAttribute("d", path3);

      requestAnimationFrame(update);
    }
    update();
  </script>
</div>
"##;

pub const GRADIENT_WAVE_CSS: &str = r##".custom-ocean-card {
  width: 320px;
  height: 120px;
  background: rgba(18, 18, 22, 0.94);
  border: 1.2px solid rgba(255, 255, 255, 0.05);
  border-radius: 28px;
  padding: 14px 16px;
  box-sizing: border-box;
  display: flex;
  flex-direction: column;
  justify-content: space-between;
  box-shadow: 0 16px 40px rgba(0, 0, 0, 0.6), inset 0 1px 0 rgba(255, 255, 255, 0.05);
  backdrop-filter: blur(16px);
  user-select: none;
  pointer-events: none;
}

.header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  width: 100%;
}

.label {
  font-family: 'Inter', sans-serif;
  font-size: 11px;
  font-weight: 550;
  color: rgba(255, 255, 255, 0.35);
  letter-spacing: -0.01em;
}

.badge {
  display: inline-flex;
  align-items: center;
  border: 1px solid rgba(16, 185, 129, 0.4);
  background: rgba(16, 185, 129, 0.07);
  color: #10b981;
  font-family: 'Inter', sans-serif;
  font-size: 10.5px;
  font-weight: 600;
  border-radius: 6px;
  padding: 2.5px 9px;
  max-width: 130px;
}

.ocean {
  display: flex;
  align-items: flex-end;
  justify-content: center;
  height: 55px;
  width: 100%;
  margin-top: auto;
  overflow: hidden;
  border-radius: 0 0 16px 16px;
}

svg {
  display: block;
  overflow: hidden;
}
"##;

pub const RAINBOW_HTML: &str = r##"<div class="rainbow-card">
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

  <div class="blue-bar">
    <span class="keybind-title">{{target}}</span>
  </div>

  <script>
    (function() {
      let isActive = false;
      let audioLevel = 0;
      let animationFrameId = null;

      function handleStatus(e) {
        const { recording, processing, speaking } = e.detail;
        isActive = recording || processing || speaking;
      }

      function handleAudioLevel(e) {
        // Scale the raw RMS level dynamically (e.max ~0.12) to fit a nice visual height range
        audioLevel = Math.min(1.0, e.detail * 8.0);
      }

      window.addEventListener("voxctrl-status", handleStatus);
      window.addEventListener("voxctrl-audio-level", handleAudioLevel);

      const bars = document.querySelectorAll(".rainbow-card .bar");
      const barCount = 15;
      
      // Shift register / propagation queue for history (0 is left, 14 is right)
      const history = Array(barCount).fill(0.0);
      const scrollSpeed = 0.35; // Controls wave propagation speed to the left
      let time = 0;

      function draw() {
        time += 0.05; // Increment time for breathing/vibration
        const currentInput = isActive ? audioLevel : 0.0;

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
  justify-content: center;
  width: 480px;
  height: 130px;
  box-sizing: border-box;
  pointer-events: none;
  user-select: none;
  position: relative;
}

.rainbow-card .black-capsule {
  position: relative;
  width: 440px;
  height: 72px;
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
  padding-bottom: 10px;
  box-sizing: border-box;
}

.bar {
  width: 10px;
  height: 5px;
  border-radius: 9999px;
  /* Removed CSS height transition so JS requestAnimationFrame updates are immediate and stutter-free */
}

/* Horizontal color spectrum with corresponding color glows */
.bar-1, .bar-15 { background: #2b66ff; box-shadow: 0 0 12px rgba(43, 102, 255, 0.65); }
.bar-2, .bar-14 { background: #0088ff; box-shadow: 0 0 12px rgba(0, 136, 255, 0.65); }
.bar-3, .bar-13 { background: #00bfff; box-shadow: 0 0 12px rgba(0, 191, 255, 0.65); }
.bar-4, .bar-12 { background: #00ffd5; box-shadow: 0 0 12px rgba(0, 255, 213, 0.65); }
.bar-5, .bar-11 { background: #00ff88; box-shadow: 0 0 12px rgba(0, 255, 136, 0.65); }
.bar-6, .bar-10 { background: #33ff33; box-shadow: 0 0 12px rgba(51, 255, 51, 0.65); }
.bar-7, .bar-9  { background: #ccff00; box-shadow: 0 0 12px rgba(204, 255, 0, 0.65); }
.bar-8          { background: #ffea00; box-shadow: 0 0 16px rgba(255, 234, 0, 0.85); }

.blue-bar {
  margin-top: 10px;
  background: rgba(29, 78, 216, 0.18);
  border: 1.2px solid rgba(59, 130, 246, 0.4);
  border-radius: 9999px;
  padding: 5px 28px;
  box-shadow: 0 4px 16px rgba(59, 130, 246, 0.28);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 5;
  
  /* Initial hidden state */
  opacity: 0;
  transform: translateY(12px);
  transition: opacity 0.35s ease, transform 0.4s cubic-bezier(0.16, 1, 0.3, 1);
}

/* Transitions to active state via .custom-overlay-content.active */
.custom-overlay-content.active .rainbow-card .blue-bar {
  opacity: 1;
  transform: translateY(0);
}

.keybind-title {
  font-family: 'Inter', system-ui, sans-serif;
  font-size: 13.5px;
  font-weight: 550;
  color: #ffffff;
  letter-spacing: -0.01em;
  text-shadow: 0 1px 2px rgba(0, 0, 0, 0.3);
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
