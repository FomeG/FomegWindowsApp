// Phiên bản app: lấy từ package.json (nguồn duy nhất; tauri.conf.json cũng đọc từ đó). Đổi bằng `npm run bump x.y.z`.
import { version } from '../../package.json';

export const APP_VERSION = version;
