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

    // Listen to dynamic voxctr level events
    window.addEventListener("voxctr-audio-level", (e) => {
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

pub fn ensure_default_overlays(overlays_dir: &Path) {
    let gw_dir = overlays_dir.join("gradient-wave");
    if !gw_dir.exists() {
        if let Ok(_) = std::fs::create_dir_all(&gw_dir) {
            let _ = std::fs::write(gw_dir.join("index.html"), GRADIENT_WAVE_HTML);
            let _ = std::fs::write(gw_dir.join("style.css"), GRADIENT_WAVE_CSS);
        }
    }
}
