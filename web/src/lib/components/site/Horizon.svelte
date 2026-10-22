<!-- アプリアイコン（日・水平線・波）を線画にした装飾。横幅いっぱいに伸ばし、日の位置（0〜1）だけを指定する。 -->
<script lang="ts">
  let { sun = 0.72, class: cls = '' }: { sun?: number; class?: string } = $props()
  const x = $derived(`${sun * 100}%`)
</script>

<svg class="horizon {cls}" height="120" aria-hidden="true" focusable="false">
  <svg {x} y="0" overflow="visible">
    <circle cx="0" cy="58" r="30" class="sun" />
    <rect x="-40" y="58" width="80" height="12" class="mask" />
    <path d="M-120 84q20-10 40 0t40 0t40 0t40 0t40 0t40 0" class="wave" />
    <path d="M-60 104q20-10 40 0t40 0t40 0" class="wave faint" />
  </svg>
  <line x1="0" x2="100%" y1="58" y2="58" class="line" />
</svg>

<style>
  .horizon {
    display: block;
    width: 100%;
    overflow: visible;
  }
  .sun {
    fill: var(--sun);
  }
  .mask {
    fill: var(--horizon-bg, var(--bg));
  }
  .line,
  .wave {
    fill: none;
    stroke: var(--horizon-ink, var(--fg));
    stroke-width: 1.25;
    stroke-linecap: round;
  }
  .wave {
    stroke: var(--horizon-wave, var(--accent));
    opacity: 0.75;
  }
  .faint {
    opacity: 0.4;
  }
</style>
