<script lang="ts">
  let {
    checked = false,
    label,
    disabled = false,
    onchange,
  }: {
    checked?: boolean;
    label: string;
    disabled?: boolean;
    onchange?: (value: boolean) => void;
  } = $props();
</script>

<button
  type="button"
  class="switch"
  class:on={checked}
  role="switch"
  aria-checked={checked}
  aria-label={label}
  {disabled}
  onclick={() => onchange?.(!checked)}
>
  <span class="track"><span class="thumb"></span></span>
  <span class="text">{label}</span>
</button>

<style>
  .switch {
    display: flex;
    align-items: center;
    gap: 12px;
    border: none;
    background: transparent;
    color: var(--on-surface);
    font: inherit;
    cursor: pointer;
    padding: 4px 0;
    text-align: left;
  }
  .track {
    width: 52px;
    height: 32px;
    border-radius: var(--shape-full);
    background: var(--surface-container-highest);
    box-shadow: inset 0 0 0 2px var(--outline);
    display: flex;
    align-items: center;
    padding: 4px;
    transition: background-color var(--motion) var(--ease-emph);
  }
  .thumb {
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: var(--outline);
    transition:
      transform var(--motion) var(--ease-spring),
      width var(--motion) var(--ease-emph),
      height var(--motion) var(--ease-emph),
      background-color var(--motion) var(--ease-standard);
  }
  .on .track {
    background: var(--primary);
    box-shadow: none;
  }
  .on .thumb {
    width: 24px;
    height: 24px;
    transform: translateX(20px);
    background: var(--on-primary);
  }
  .text {
    font-size: 14px;
  }
  .switch:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
</style>
