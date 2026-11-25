<script>
  // Cài đặt trợ lý AI dùng chung cho trang Cài đặt và panel trợ lý: nhà cung cấp, khoá, Base URL, model, mức suy nghĩ, công cụ.
  import { ai, PROVIDERS, EFFORTS } from './ai.svelte.js';
  import OptionRow from './OptionRow.svelte';
  import Glyph from './Glyph.svelte';
  import { t } from './i18n.svelte.js';

  const uid = $props.id();
  const provider = $derived(ai.cfg?.provider);
  const active = $derived(ai.activeSettings);
  const modelList = $derived(ai.modelLists[provider]);
  let saving = $state('');

  function commitOnEnter(e) {
    if (e.key === 'Enter' && !e.isComposing) e.currentTarget.blur();
  }
  function saveField(target, field, e) {
    const input = e.currentTarget;
    input.value = input.value.trim();
    ai.saveProvider(target, { [field]: input.value });
  }
  async function saveKey(target, e) {
    const input = e.currentTarget;
    const typed = input.value;
    const api_key = typed.trim();
    if (api_key && await ai.saveProvider(target, { api_key }) && input.value === typed) input.value = '';
  }
  async function setCfg(key, v) {
    saving = key;
    await ai.setConfig({ [key]: v });
    saving = '';
  }
</script>

{#if active}
  <div class="opt-list">
    <OptionRow glyph="chat" title={t('settings.ai.provider')} desc={t('settings.ai.provider_desc')}>
      <div class="seg ai-providers" role="group" aria-label={t('settings.ai.provider')}>
        {#each Object.entries(PROVIDERS) as [id, label] (id)}
          <button type="button" aria-pressed={provider === id} class:active={provider === id} onclick={() => ai.switchProvider(id)}>{label}</button>
        {/each}
      </div>
    </OptionRow>

    {#key provider}
      {@const target = provider}
      <OptionRow glyph="key" title={target === 'custom' ? t('settings.ai.key_optional') : 'API key'} desc={t('settings.ai.key_desc')}>
        {#snippet below()}
          {#if active.has_key}<span class="opt-ok small"><Glyph name="check" size={12} /> {t('settings.ai.key_saved')}</span>{/if}
        {/snippet}
        <input id="{uid}-key" type="password" class="inp" aria-label="API key" placeholder={active.has_key ? t('settings.ai.key_replace') : t('settings.ai.key_enter')} autocomplete="off" spellcheck="false" onchange={(e) => saveKey(target, e)} onkeydown={commitOnEnter} />
        {#if active.has_key}
          <button type="button" class="btn sm danger" onclick={() => ai.saveProvider(target, { api_key: '' })}><Glyph name="delete" size={12} /> {t('settings.ai.key_delete')}</button>
        {/if}
      </OptionRow>

      <OptionRow glyph="network" title={target === 'custom' ? t('settings.ai.base_required') : 'Base URL'} desc={target === 'custom' ? 'LM Studio, Ollama (http://localhost:11434/v1), OpenRouter, proxy…' : t('settings.ai.base_desc')}>
        <input id="{uid}-base" type="url" class="inp" aria-label="Base URL" value={active.base_url} placeholder={target === 'custom' ? 'http://localhost:1234/v1' : active.default_base_url} required={target === 'custom'} spellcheck="false" onchange={(e) => saveField(target, 'base_url', e)} onkeydown={commitOnEnter} />
      </OptionRow>

      <OptionRow glyph="robot" title="Model" desc={t('settings.ai.model_desc')}>
        {#snippet below()}
          <div class="ai-model-actions">
            <button type="button" class="btn xs" disabled={modelList?.loading} onclick={() => ai.listModels(target)}>
              {#if modelList?.loading && !modelList.check}<span class="spin xs" aria-hidden="true"></span>{/if} {t('settings.ai.load_models')}
            </button>
            <button type="button" class="btn xs" disabled={modelList?.loading} onclick={() => ai.listModels(target, true)}>
              {#if modelList?.loading && modelList.check}<span class="spin xs" aria-hidden="true"></span>{/if} {t('settings.ai.test')}
            </button>
          </div>
          <span class="small" role="status">
            {#if modelList?.loading}
              <span class="muted">{t('settings.loading')}</span>
            {:else if modelList?.error}
              <span class="ai-error">{modelList.check ? '✗ ' : ''}{modelList.error}</span>
            {:else if modelList?.models}
              {#if modelList.check}<span class="ai-connected">✓ {t('settings.ai.connected')} · </span>{/if}
              <span class="muted">{t('settings.ai.models', { count: modelList.models.length })}</span>
            {/if}
          </span>
        {/snippet}
        <input id="{uid}-model" type="text" class="inp" aria-label="Model" value={active.model} list="{uid}-models" placeholder={t('settings.ai.model_ph')} autocomplete="off" spellcheck="false" onchange={(e) => saveField(target, 'model', e)} onkeydown={commitOnEnter} />
        <datalist id="{uid}-models">
          {#each modelList?.models as model}<option value={model}></option>{/each}
        </datalist>
      </OptionRow>

      {#if target === 'anthropic'}
        <OptionRow glyph="speed" title={t('settings.ai.effort')} desc={t('settings.ai.effort_desc')}>
          <select class="inp" aria-label={t('settings.ai.effort')} value={active.effort || 'medium'} onchange={(e) => saveField(target, 'effort', e)}>
            {#each EFFORTS as [id, label] (id)}<option value={id}>{label}</option>{/each}
          </select>
        </OptionRow>
      {/if}
    {/key}

    <OptionRow glyph="search" title={t('settings.ai.tools')} desc={t('settings.ai.tools_desc')}
      on={ai.cfg.tools !== false} pending={saving === 'tools'} onchange={(v) => setCfg('tools', v)} />
    <OptionRow glyph="pulse" title={t('settings.ai.context')} desc={t('settings.ai.context_desc')}
      on={!!ai.cfg.include_context} pending={saving === 'include_context'} onchange={(v) => setCfg('include_context', v)} />
  </div>
{:else if ai.loadError}
  <p class="ai-error" role="alert">{t('settings.ai.load_error', { error: ai.loadError })}</p>
  <button type="button" class="btn" onclick={() => ai.loadConfig()}>{t('settings.retry')}</button>
{:else}
  <p class="muted" role="status">{t('settings.ai.loading_cfg')}</p>
{/if}
