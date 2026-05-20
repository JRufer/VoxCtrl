<script lang="ts">
  let { recording = false } = $props();
</script>

<div class="pulse-container" class:active={recording}>
  <div class="ring ring1"></div>
  <div class="ring ring2"></div>
  <div class="core"></div>
</div>

<style>
  .pulse-container {
    position: relative;
    width: 80px;
    height: 80px;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .ring {
    position: absolute;
    border-radius: 50%;
    border: 2px solid var(--recording-color, #e94560);
    opacity: 0;
  }

  .active .ring {
    animation: expand 2s ease-out infinite;
  }

  .ring1 { width: 100%; height: 100%; }
  .ring2 { width: 100%; height: 100%; animation-delay: 1s !important; }

  .core {
    width: 24px;
    height: 24px;
    border-radius: 50%;
    background: var(--recording-color, #e94560);
    transition: background 0.3s;
  }

  @keyframes expand {
    0%   { opacity: 0.8; transform: scale(0.3); }
    100% { opacity: 0;   transform: scale(1.5); }
  }
</style>
