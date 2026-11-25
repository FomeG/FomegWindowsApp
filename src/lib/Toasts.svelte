<script>
  import { toasts } from './toasts.svelte.js';
  import Icon from './Icon.svelte';
  import { t } from './i18n.svelte.js';
  const ICON = { success: 'check', error: 'alert', warn: 'alert', info: 'info' };
</script>

<div class="toasts" aria-live="polite">
  {#each toasts.list as toast (toast.id)}
    <div class="toast t-{toast.kind}" role={toast.kind === 'error' ? 'alert' : 'status'}>
      <span class="t-icon"><Icon name={ICON[toast.kind]} size={16} /></span>
      <div class="t-body">
        <div class="t-title">{toast.title}</div>
        {#if toast.details?.length}
          {#if toast.details.length === 1}
            <div class="t-detail">{toast.details[0]}</div>
          {:else}
            <details>
              <summary>{t('shell.toast.details', { n: toast.details.length })}</summary>
              <ul>
                {#each toast.details as d, i (i)}<li>{d}</li>{/each}
              </ul>
            </details>
          {/if}
        {/if}
      </div>
      <button type="button" class="icon-btn xs" aria-label={t('shell.toast.close')} title={t('shell.close')} onclick={() => toasts.dismiss(toast.id)}>
        <Icon name="close" size={13} />
      </button>
    </div>
  {/each}
</div>
