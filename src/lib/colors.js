// Màu theo tài nguyên (giống RustWTM / Task Manager). Dạng "r,g,b" để ghép alpha: `rgba(${c},.3)`.
export const C = {
  cpu: '56,139,230',
  mem: '160,108,213',
  gpu: '63,185,80',
  read: '63,185,80',
  write: '232,83,74',
  disk: '63,185,80',
  rx: '32,178,170',
  tx: '230,140,50',
  net: '32,178,170',
  bat: '214,170,40',
  ded: '63,185,80',
  shared: '120,170,90',
};
export const rgb = (c) => `rgb(${c})`;
