// Store trợ lý AI theo contract v3: cấu hình theo nhà cung cấp và luồng trò chuyện.
import * as api from './api.js';
import { prefs } from './prefs.svelte.js';
import { errText } from './format.js';
import { du } from './du.svelte.js';
import { store } from './store.svelte.js';
import { cleanup } from './cleanup.svelte.js';
import { t, labels } from './i18n.svelte.js';

export const PROVIDERS = labels({
  anthropic: 'ai.provider.anthropic',
  openai: 'ai.provider.openai',
  custom: 'ai.provider.custom',
});
// [id, nhãn]: phần tử [1] là getter nên `{#each EFFORTS as [id, label]}` đổi theo ngôn ngữ
export const EFFORTS = ['low', 'medium', 'high', 'xhigh', 'max'].map((id) => {
  const pair = [id];
  Object.defineProperty(pair, 1, { get: () => t(`ai.effort.${id}`), enumerable: true });
  return pair;
});

let seq = 0;
/** Danh sách cổng gửi lại sau khoảng này (ms) nếu vẫn hỏi từ trang Cổng. */
const PORTS_TTL = 60000;
const newChatId = () => 'c' + Date.now().toString(36) + Math.random().toString(36).slice(2, 6);

class Ai {
  cfg = $state(null);
  loadError = $state('');
  saveStatus = $state(null);
  modelLists = $state({});
  chatId = $state(newChatId());
  /** { id, role: 'user'|'assistant'|'error', text, streaming?, thinking?, stop? } */
  msgs = $state([]);
  busy = $state(false); // đang chờ / đang stream
  draft = $state('');
  showSettings = $state(false);
  focusTick = $state(0); // tăng để yêu cầu focus ô nhập
  /** popup xem lại đề xuất hành động (ActionReview): { kind: 'xoa' | 'dong_cong' | …, items } */
  review = $state(null);

  // Kết quả quét ổ cứng chỉ gửi kèm khi cần (đang ở trang Ổ cứng hoặc hỏi về một thư mục) và không gửi lại
  // trong cùng cuộc trò chuyện (model đã có trong lịch sử) -> ít token. Gửi lại khi có lần quét mới / quét xong,
  // hoặc khi hỏi về một thư mục chưa hỏi (bản tóm tắt nào cũng đã có phần tổng quan).
  #diskScan = ''; // "scan|xong" đã gửi
  #diskFocus = new Set(); // các thư mục đã gửi chi tiết (chữ thường)
  #diskPending = null; // { scan, focus } chờ lượt trả lời thành công
  #focus = null; // thư mục cho câu hỏi đang soạn (từ "Hỏi trợ lý về thư mục này")
  // Danh sách cổng: gửi kèm khi hỏi từ trang Cổng. Cổng đổi liên tục nên gửi lại nếu lần trước đã quá PORTS_TTL,
  // hoặc khi bấm "Hỏi AI" của trang Cổng (#portsForce). Trong khoảng đó model dùng bản đã có trong lịch sử.
  #portsAt = 0; // lúc gửi thành công gần nhất
  #portsForce = false;
  #portsPending = false;
  // Kết quả trang Dọn dẹp: gửi lại khi có lần quét mới / có mục vừa đo xong, hoặc khi bấm "Hỏi AI" (#cleanupForce).
  #cleanupSent = '';
  #cleanupForce = false;
  #cleanupPending = '';

  #started = false;
  #cur = null; // tin nhắn assistant đang stream
  #configWrites = Promise.resolve();
  #pendingSaves = $state(0);

  get activeSettings() {
    return this.cfg?.providers[this.cfg.provider];
  }

  get subtitle() {
    if (!this.cfg) return '';
    return `${PROVIDERS[this.cfg.provider]} · ${this.activeSettings.model || t('ai.no_model')}`;
  }

  get missingSettings() {
    if (!this.cfg) return [];
    const { provider } = this.cfg;
    const { model, has_key, base_url } = this.activeSettings;
    return [
      ...(!has_key && provider !== 'custom' ? ['API key'] : []),
      ...(!base_url && provider === 'custom' ? ['Base URL'] : []),
      ...(!model ? ['model'] : []),
    ];
  }

  get ready() {
    return !!this.cfg && this.missingSettings.length === 0;
  }

  get saving() {
    return this.#pendingSaves > 0;
  }

  init() {
    if (this.#started) return;
    this.#started = true;
    api.listen('ai', (e) => this.onEvent(e)).catch(() => {});
    this.loadConfig();
  }

  async loadConfig() {
    this.loadError = '';
    try {
      this.cfg = await api.aiGetConfig();
    } catch (e) {
      this.loadError = errText(e);
    }
  }

  setConfig(patch) {
    this.#pendingSaves++;
    const save = this.#configWrites.then(async () => {
      try {
        const before = this.cfg;
        const updated = await api.aiSetConfig(patch);
        const target = patch.target ?? updated.provider;
        if (patch.api_key !== undefined || before?.providers[target].base_url !== updated.providers[target].base_url) {
          delete this.modelLists[target];
        }
        this.cfg = updated;
        this.saveStatus = { text: t('ai.saved') };
        return true;
      } catch (e) {
        this.saveStatus = { text: errText(e), error: true };
        return false;
      } finally {
        this.#pendingSaves--;
      }
    });
    this.#configWrites = save;
    return save;
  }

  switchProvider(provider) {
    return this.setConfig({ provider });
  }

  saveProvider(target, patch) {
    return this.setConfig({ ...patch, target });
  }

  async listModels(provider, check = false) {
    await this.#configWrites;
    if (this.modelLists[provider]?.loading) return;
    this.modelLists[provider] = {
      models: this.modelLists[provider]?.models ?? null,
      loading: true,
      error: '',
      check,
    };
    // Nếu URL hoặc khoá đổi trong lúc tải, kết quả cũ không đi vào cache mới.
    const result = this.modelLists[provider];
    try {
      result.models = await api.aiListModels(provider);
    } catch (e) {
      result.error = errText(e);
    } finally {
      result.loading = false;
    }
  }

  toggle(open = !prefs.aiOpen) {
    prefs.set('aiOpen', open);
    if (open) { this.init(); this.focusTick++; }
  }

  /** Mở panel và GỬI NGAY câu hỏi (nút "Hỏi AI" của các trang). Cấu hình chưa tải (panel chưa mở lần nào) thì chờ tải
   *  xong rồi gửi; đang có lượt trả lời / chưa thiết lập khoá thì điền sẵn vào ô nhập để người dùng tự gửi. */
  async askNow(q) {
    this.toggle(true);
    this.showSettings = false;
    if (!this.cfg && !this.loadError) await this.loadConfig();
    if (this.ready && !this.busy && !this.saving) this.send(q);
    else this.ask(q);
  }

  /** Mở panel và điền sẵn câu hỏi (không tự gửi). */
  ask(text) {
    this.toggle(true);
    this.showSettings = false;
    this.draft = text;
    this.focusTick++;
  }

  /** Có kết quả quét ổ cứng để gửi kèm câu hỏi này không (null = không). */
  #diskFor(focus) {
    if (!du.scan || (!focus && store.page !== 'disk')) return null;
    const scan = `${du.scan}|${du.progress?.done ? 1 : 0}`;
    const f = focus ? focus.toLowerCase() : '';
    if (scan === this.#diskScan && (!f || this.#diskFocus.has(f))) return null;
    this.#diskPending = { scan, focus: f };
    return { focus: focus || null };
  }

  /** Hỏi về một thư mục của trang Ổ cứng: gửi ngay nếu được, không thì điền sẵn câu hỏi. */
  askFolder(path) {
    const q = t('ai.ask.folder', { path });
    this.#focus = path;
    this.askNow(q);
  }

  /** Số liệu ổ đĩa đã đổi (vừa xoá bớt): lần hỏi sau gửi lại bản tóm tắt mới. */
  diskChanged() {
    this.#diskScan = '';
    this.#diskFocus.clear();
  }

  get diskSuggest() {
    return !!du.scan && store.page === 'disk';
  }
  get portsSuggest() {
    return store.page === 'ports';
  }

  /** Nút "Hỏi AI" của trang Cổng: gửi ngay câu hỏi chung kèm danh sách cổng mới nhất. */
  askPorts(q = t('ai.ask.ports')) {
    this.#portsForce = true;
    this.askNow(q);
  }
  #portsFor() {
    if (store.page !== 'ports') return false;
    const force = this.#portsForce;
    this.#portsForce = false;
    return force || Date.now() - this.#portsAt > PORTS_TTL;
  }

  get appsSuggest() {
    return store.page === 'apps';
  }
  /** Nút "Hỏi AI" của trang Ứng dụng: trợ lý tự xem danh sách bằng tool installed_apps (không gửi kèm khối dữ liệu). */
  askApps(q = t('ai.ask.apps')) {
    this.askNow(q);
  }

  get cleanupSuggest() {
    return store.page === 'cleanup';
  }
  /** Nút "Hỏi AI" của trang Dọn dẹp: gửi ngay câu hỏi kèm kết quả quét mới nhất. */
  askCleanup(q = t('ai.ask.cleanup')) {
    this.#cleanupForce = true;
    this.askNow(q);
  }
  #cleanupFor() {
    const force = this.#cleanupForce;
    this.#cleanupForce = false;
    if (store.page !== 'cleanup' || !cleanup.scanId) return false;
    const key = `${cleanup.scanId}|${cleanup.pendingCount}`;
    if (!force && key === this.#cleanupSent) return false;
    this.#cleanupPending = key;
    return true;
  }

  async send(text = this.draft) {
    text = (text || '').trim();
    if (!text || this.busy || !this.ready || this.saving) return;
    const disk = this.#diskFor(this.#focus);
    const ports = this.#portsFor();
    this.#portsPending = ports;
    const clean = this.#cleanupFor();
    this.#focus = null;
    this.draft = '';
    this.msgs.push({ id: ++seq, role: 'user', text, disk: !!disk, ports, cleanup: clean });
    this.msgs.push({ id: ++seq, role: 'assistant', text: '', streaming: true, thinking: false });
    this.#cur = this.msgs[this.msgs.length - 1];
    this.busy = true;
    try {
      await api.aiSend(this.chatId, text, disk, ports, clean);
    } catch (e) {
      this.#fail(errText(e));
    }
  }

  onEvent(e) {
    if (!e || e.chat_id !== this.chatId) return;
    // Đang huỷ: bỏ qua chữ đến muộn, chỉ chờ sự kiện kết thúc để mở khoá ô nhập
    // (tránh lượt cũ kết thúc nhầm lượt mới gửi ngay sau khi bấm Dừng).
    if (this.#cancelling) {
      if (e.kind === 'done' || e.kind === 'error') this.#endCancel();
      return;
    }
    const m = this.#cur;
    switch (e.kind) {
      case 'thinking':
        if (m) m.thinking = true;
        break;
      case 'delta':
        if (m) { m.thinking = false; m.text += e.text || ''; }
        break;
      case 'tool': {
        // trợ lý tự tra cứu bằng tool chỉ đọc: start -> đang xem, done -> xong (ok / lỗi)
        if (!m) break;
        m.thinking = false;
        if (!m.tools) m.tools = [];
        const t = m.tools.find((x) => x.id === e.id);
        const step = { id: e.id, name: e.name, label: e.label, args: e.args, done: e.phase === 'done', ok: !!e.ok, chars: e.chars || 0 };
        if (t) Object.assign(t, step);
        else m.tools.push(step);
        break;
      }
      case 'done':
        if (m) {
          m.streaming = false;
          m.thinking = false;
          m.stop = e.stop_reason;
          m.usage = e.usage || null;
          if (!m.text && e.stop_reason === 'cancelled') m.text = t('ai.stopped');
        }
        if (e.stop_reason !== 'cancelled' && this.#diskPending) {
          const { scan, focus } = this.#diskPending;
          if (scan !== this.#diskScan) { this.#diskScan = scan; this.#diskFocus.clear(); }
          if (focus) this.#diskFocus.add(focus);
        }
        this.#diskPending = null;
        if (e.stop_reason !== 'cancelled' && this.#portsPending) this.#portsAt = Date.now();
        this.#portsPending = false;
        if (e.stop_reason !== 'cancelled' && this.#cleanupPending) this.#cleanupSent = this.#cleanupPending;
        this.#cleanupPending = '';
        this.#cur = null;
        this.busy = false;
        break;
      case 'error':
        this.#fail(e.message || t('common.unknown_error'));
        break;
    }
  }

  #fail(msg) {
    this.#diskPending = null;
    this.#portsPending = false;
    this.#cleanupPending = '';
    const m = this.#cur;
    if (m) {
      m.streaming = false;
      m.thinking = false;
      if (!m.text) { const i = this.msgs.indexOf(m); if (i >= 0) this.msgs.splice(i, 1); }
    }
    this.msgs.push({ id: ++seq, role: 'error', text: msg });
    this.#cur = null;
    this.busy = false;
  }

  #cancelling = false;
  #cancelTimer = null;

  #endCancel() {
    this.#cancelling = false;
    clearTimeout(this.#cancelTimer);
    this.busy = false;
  }

  async cancel() {
    const id = this.chatId;
    // kết thúc phía giao diện ngay, không phụ thuộc backend có gửi `done` hay không
    const m = this.#cur;
    if (m) {
      m.streaming = false;
      m.thinking = false;
      m.stop = 'cancelled';
      if (!m.text) m.text = t('ai.stopped');
    }
    this.#cur = null;
    // Giữ busy cho tới khi backend báo kết thúc lượt (tối đa 15 s phòng khi mất sự kiện).
    this.#cancelling = true;
    clearTimeout(this.#cancelTimer);
    this.#cancelTimer = setTimeout(() => this.#endCancel(), 15000);
    try { await api.aiCancel(id); } catch { this.#endCancel(); }
  }

  async newChat() {
    const old = this.chatId;
    if (this.busy) await this.cancel();
    this.#endCancel(); // cuộc trò chuyện mới dùng chat_id mới, không cần chờ lượt cũ
    api.aiReset(old).catch(() => {});
    this.chatId = newChatId();
    this.msgs = [];
    this.#diskScan = '';
    this.#diskFocus.clear();
    this.#diskPending = null;
    this.#portsAt = 0;
    this.#portsPending = false;
    this.#cleanupSent = '';
    this.#cleanupPending = '';
    this.draft = '';
    this.focusTick++;
  }
}

export const ai = new Ai();

// Store dùng chung (một bản duy nhất): sửa file khi dev -> tải lại cả trang, tránh hai bản cũ / mới cùng chạy sau HMR.
if (import.meta.hot) import.meta.hot.decline();
