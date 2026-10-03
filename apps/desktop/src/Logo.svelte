<script lang="ts">
  import mark from '../../../assets/brand/mark.svg';
  let { size = 20 }: { size?: number } = $props();
</script>

<!-- Vite's inlined SVG URLs contain single quotes; quote the CSS URL safely. -->
<span aria-hidden="true" style:width={`${size}px`} style:height={`${size}px`} style={`--logo-url: url(${JSON.stringify(mark)})`}></span>

<style>
  span { display: inline-block; flex-shrink: 0; background: currentColor; -webkit-mask: var(--logo-url) center / contain no-repeat; mask: var(--logo-url) center / contain no-repeat; }
</style>
