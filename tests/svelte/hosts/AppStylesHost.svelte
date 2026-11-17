<!-- Holds the rules in $state, as FeaturesTab's config does, so a test sees
     what the editor shows after its own changes. -->
<script lang="ts">
  import AppStylesEditor from "../../../src/lib/Settings/AppStylesEditor.svelte";
  import type { AppStyleRule } from "../../../src/stores/config";

  let { initial, options }: { initial: AppStyleRule[]; options: { value: string; label: string }[] } = $props();
  // svelte-ignore state_referenced_locally
  let rules = $state(initial);
</script>

<AppStylesEditor bind:rules styleOptions={options} />
<pre data-testid="rules">{JSON.stringify(rules)}</pre>
