<script lang="ts">
  let { recording = false, speaking = false } = $props();

  let label = $derived(
    recording ? "Listening…" : speaking ? "Speaking…" : ""
  );
  let color = $derived(recording ? "#e94560" : "#4fc3f7");
</script>

<div class="card" style="--dot-color: {color}">
  <span class="dot" class:pulse={recording || speaking}></span>
  <span class="label">{label}</span>
</div>

<style>
  .card {
    display: flex;
    align-items: center;
    gap: 10px;
    background: rgba(15, 15, 25, 0.88);
    backdrop-filter: blur(12px);
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: 24px;
    padding: 12px 20px;
    box-shadow: 0 4px 24px rgba(0, 0, 0, 0.5);
    color: #e0e0e0;
    font-size: 15px;
    font-weight: 500;
    pointer-events: none;
  }

  .dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--dot-color);
    flex-shrink: 0;
  }

  .dot.pulse {
    animation: pulse 1.2s ease-in-out infinite;
  }

  @keyframes pulse {
    0%, 100% { opacity: 1; transform: scale(1); }
    50% { opacity: 0.5; transform: scale(1.3); }
  }
</style>
