// Danh mục cổng quen thuộc của công cụ lập trình / máy chủ (theo RustWTM wtm-core/ports.rs) + tiện ích trang Cổng.
// Tên công cụ là tên riêng nên giữ nguyên; nhóm và vài mô tả được dịch (getter, đổi theo ngôn ngữ).
import { t, labels } from './i18n.svelte.js';

export const CAT = labels({
  dev: 'ports.cat.dev',
  db: 'ports.cat.db',
  cache: 'ports.cat.cache',
  search: 'ports.cat.search',
  cloud: 'ports.cat.cloud',
  mon: 'ports.cat.mon',
  ai: 'ports.cat.ai',
  debug: 'ports.cat.debug',
  web: 'ports.cat.web',
  sys: 'ports.cat.sys',
});

// [cổng, công cụ, nhóm] – thứ tự là thứ tự hiển thị. Công cụ dạng { key } = mô tả cần dịch.
const RAW = [
  [3000, 'Node / React / Next.js / Grafana', 'dev'],
  [3001, { key: 'ports.tool.node2' }, 'dev'],
  [4200, 'Angular CLI', 'dev'],
  [5173, 'Vite', 'dev'],
  [5174, { key: 'ports.tool.vite2' }, 'dev'],
  [4173, 'Vite preview', 'dev'],
  [8080, 'HTTP alt / webpack / Tomcat / Spring', 'dev'],
  [8000, 'Django / Python http.server / Laravel', 'dev'],
  [5000, 'Flask / ASP.NET / Docker registry', 'dev'],
  [5001, 'ASP.NET (HTTPS)', 'dev'],
  [7071, 'Azure Functions', 'dev'],
  [8081, 'Metro (React Native)', 'dev'],
  [8787, 'Wrangler (Cloudflare Workers)', 'dev'],
  [19000, 'Expo', 'dev'],
  [6006, 'Storybook', 'dev'],
  [4321, 'Astro', 'dev'],
  [4000, 'Phoenix / Jekyll / Gatsby', 'dev'],
  [1313, 'Hugo', 'dev'],
  [1420, 'Tauri (Vite dev)', 'dev'],
  [51204, 'Vitest UI', 'dev'],
  [9323, 'Playwright report', 'dev'],
  [5432, 'PostgreSQL', 'db'],
  [3306, 'MySQL / MariaDB', 'db'],
  [1433, 'SQL Server', 'db'],
  [1521, 'Oracle', 'db'],
  [27017, 'MongoDB', 'db'],
  [9042, 'Cassandra / ScyllaDB', 'db'],
  [7474, 'Neo4j', 'db'],
  [8086, 'InfluxDB', 'db'],
  [8123, 'ClickHouse (HTTP)', 'db'],
  [5984, 'CouchDB', 'db'],
  [26257, 'CockroachDB', 'db'],
  [6333, 'Qdrant', 'db'],
  [8529, 'ArangoDB', 'db'],
  [6379, 'Redis / Valkey', 'cache'],
  [11211, 'Memcached', 'cache'],
  [5672, 'RabbitMQ', 'cache'],
  [15672, 'RabbitMQ management', 'cache'],
  [9092, 'Kafka', 'cache'],
  [2181, 'ZooKeeper', 'cache'],
  [4222, 'NATS', 'cache'],
  [1883, 'MQTT', 'cache'],
  [9200, 'Elasticsearch / OpenSearch', 'search'],
  [5601, 'Kibana', 'search'],
  [7700, 'Meilisearch', 'search'],
  [8108, 'Typesense', 'search'],
  [2375, 'Docker API', 'cloud'],
  [2376, 'Docker API (TLS)', 'cloud'],
  [6443, 'Kubernetes API', 'cloud'],
  [8001, 'kubectl proxy', 'cloud'],
  [9000, 'MinIO / Portainer / PHP-FPM', 'cloud'],
  [4566, 'LocalStack', 'cloud'],
  [10000, 'Azurite (Azure storage)', 'cloud'],
  [8200, 'Vault', 'cloud'],
  [8500, 'Consul', 'cloud'],
  [9090, 'Prometheus', 'mon'],
  [9100, 'Prometheus node exporter', 'mon'],
  [16686, 'Jaeger', 'mon'],
  [4317, 'OpenTelemetry (gRPC)', 'mon'],
  [4318, 'OpenTelemetry (HTTP)', 'mon'],
  [11434, 'Ollama', 'ai'],
  [1234, 'LM Studio', 'ai'],
  [7860, 'Gradio / Stable Diffusion web UI', 'ai'],
  [8188, 'ComfyUI', 'ai'],
  [8888, 'Jupyter', 'ai'],
  [9229, 'Node.js inspector', 'debug'],
  [9222, 'Chrome DevTools', 'debug'],
  [5037, 'Android Debug Bridge', 'debug'],
  [8025, 'Mailpit / MailHog (web)', 'debug'],
  [1025, 'Mailpit / MailHog (SMTP)', 'debug'],
  [80, 'HTTP', 'web'],
  [443, 'HTTPS', 'web'],
  [8443, 'HTTPS alt', 'web'],
  [22, 'SSH', 'sys'],
  [3389, 'Remote Desktop', 'sys'],
  [5900, 'VNC', 'sys'],
  [5985, 'WinRM', 'sys'],
  [445, { key: 'ports.tool.smb' }, 'sys'],
  [135, 'Windows RPC', 'sys'],
];

/** Sự kiện window: cổng vừa bị đóng ở nơi khác (popup đề xuất của trợ lý) -> trang Cổng đọc lại. */
export const PORTS_CHANGED = 'fomeg:ports-changed';

export const KNOWN = RAW.map(([port, tool, cat]) =>
  typeof tool === 'string' ? { port, tool, cat } : { port, cat, get tool() { return t(tool.key); } });
const byPort = new Map(KNOWN.map((k) => [k.port, k]));
export const known = (port) => byPort.get(port) || null;

// Nhóm thường nói HTTP -> nút "Mở" có nghĩa.
const BROWSABLE = new Set(['dev', 'web', 'mon', 'search', 'ai']);
export const browsable = (port, proto) => proto === 'tcp' && (!byPort.has(port) || BROWSABLE.has(byPort.get(port).cat));

// REACH[x] = [nhãn, giải thích] (getter: đổi theo ngôn ngữ)
export const REACH = Object.defineProperties({}, Object.fromEntries(['local', 'network', 'all'].map((k) => [
  k, { enumerable: true, get: () => [t(`ports.reach.${k}`), t(`ports.reach.${k}_tip`)] },
])));
export const REACH_ORDER = { local: 0, network: 1, all: 2 };

/** "3000", ":5173", "localhost:8080" -> số cổng (1..65535) hoặc null. */
export function parsePort(text) {
  const s = String(text || '').trim();
  if (!s) return null;
  const d = s.split(':').pop().trim();
  if (!/^\d{1,5}$/.test(d)) return null;
  const n = Number(d);
  return n >= 1 && n <= 65535 ? n : null;
}

/** Vì sao không nên kết thúc tiến trình giữ cổng từ trang Cổng (null = được). */
export function endBlocked(pid, name) {
  const n = (name || '').toLowerCase();
  if (pid === 0 || pid === 4 || n === 'system' || n === 'system idle process')
    return t('ports.block.system');
  if (n === 'svchost.exe') return t('ports.block.svchost');
  if (['lsass.exe', 'wininit.exe', 'services.exe', 'csrss.exe', 'smss.exe'].includes(n)) return t('ports.block.critical');
  if (!name) return t('ports.block.unknown');
  return null;
}

/** Cảnh báo cho tiến trình chỉ chuyển tiếp cổng hộ thứ khác. */
export function relayHint(name) {
  const n = (name || '').toLowerCase();
  if (n === 'wslrelay.exe') return t('ports.relay.wsl');
  if (n === 'com.docker.backend.exe' || n === 'vpnkit.exe' || n === 'com.docker.proxy.exe')
    return t('ports.relay.docker');
  return null;
}
