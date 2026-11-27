// Từ điển en: gộp các phần theo khu vực (mỗi khu vực một tệp để sửa song song không đụng nhau).
import common from './common.js';
import shell from './shell.js';
import settings from './settings.js';
import procs from './procs.js';
import perf from './perf.js';
import apps from './apps.js';
import disk from './disk.js';
import cleanup from './cleanup.js';
import startup from './startup.js';
import ai from './ai.js';
import ports from './ports.js';
import update from './update.js';
import env from './env.js';

export default { ...common, ...shell, ...settings, ...procs, ...perf, ...apps, ...disk, ...cleanup, ...startup, ...ai, ...ports, ...update, ...env };
