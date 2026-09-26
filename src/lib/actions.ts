/**
 * Svelte action: grow a textarea to fit its content as the user types, and
 * on mount (or when the bound value changes, via `update`).
 */
export function autoResize(node: HTMLTextAreaElement) {
  function resize() {
    node.style.height = "auto";
    node.style.height = `${node.scrollHeight}px`;
  }
  node.addEventListener("input", resize);
  const timer = setTimeout(resize, 0);

  return {
    update() {
      resize();
    },
    destroy() {
      clearTimeout(timer);
      node.removeEventListener("input", resize);
    },
  };
}
