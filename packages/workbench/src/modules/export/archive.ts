import type { Archive, ExportReport, ZipEntry } from './types.ts';

export const MAX_ARCHIVE_BYTES = 10 * 1000 * 1000;

function crc32(buffer: Buffer) {
  var crc = 0xffffffff;
  for (var i = 0; i < buffer.length; i += 1) {
    crc ^= buffer[i];
    for (var bit = 0; bit < 8; bit += 1) crc = (crc >>> 1) ^ ((crc & 1) ? 0xedb88320 : 0);
  }
  return (crc ^ 0xffffffff) >>> 0;
}

/* Stored ZIP entries avoid adding a runtime/native dependency to the extension. */
export function zip(entries: readonly ZipEntry[]): Buffer {
  var local: Buffer[] = [];
  var central: Buffer[] = [];
  var offset = 0;
  entries.forEach(function (entry) {
    var name = Buffer.from(entry.name.replace(/\\/g, '/'));
    var body = Buffer.isBuffer(entry.body) ? entry.body : Buffer.from(entry.body);
    var crc = crc32(body);
    var header = Buffer.alloc(30);
    header.writeUInt32LE(0x04034b50, 0); header.writeUInt16LE(20, 4); header.writeUInt16LE(0x0800, 6);
    header.writeUInt32LE(crc, 14); header.writeUInt32LE(body.length, 18); header.writeUInt32LE(body.length, 22);
    header.writeUInt16LE(name.length, 26);
    local.push(header, name, body);
    var record = Buffer.alloc(46);
    record.writeUInt32LE(0x02014b50, 0); record.writeUInt16LE(20, 4); record.writeUInt16LE(20, 6); record.writeUInt16LE(0x0800, 8);
    record.writeUInt32LE(crc, 16); record.writeUInt32LE(body.length, 20); record.writeUInt32LE(body.length, 24);
    record.writeUInt16LE(name.length, 28); record.writeUInt32LE(offset, 42);
    central.push(record, name);
    offset += header.length + name.length + body.length;
  });
  var centralBody = Buffer.concat(central);
  var end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0); end.writeUInt16LE(entries.length, 8); end.writeUInt16LE(entries.length, 10);
  end.writeUInt32LE(centralBody.length, 12); end.writeUInt32LE(offset, 16);
  return Buffer.concat(local.concat([centralBody, end]));
}

export function zipSize(entries: readonly ZipEntry[]): number {
  return 22 + entries.reduce(function (total, entry) {
    var name = Buffer.byteLength(entry.name.replace(/\\/g, '/'));
    var body = Buffer.isBuffer(entry.body) ? entry.body.length : Buffer.byteLength(entry.body);
    return total + body + 76 + name * 2;
  }, 0);
}

export function splitArchives(prefix: string, payload: ZipEntry[], readme: string, report: ExportReport, maximum?: number): Archive[] {
  maximum = Number(maximum) || MAX_ARCHIVE_BYTES;
  if (maximum < 1024) throw new Error('Export ZIP limit must be at least 1024 bytes');
  var groups: ZipEntry[][] = [];
  var current: ZipEntry[] = [];
  var reportReserve = Math.max(32768, payload.length * 160);
  var baseReport = JSON.stringify(Object.assign({}, report, { parts: [] }), null, 2) + '\n';

  function provisional(group: ZipEntry[]) {
    var listed = JSON.stringify({ part: 9999, of: 9999, files: group.map(function (entry) {
      return entry.name.slice(prefix.length + 1);
    }) }, null, 2) + '\n';
    return zipSize([
      { name: prefix + '/README.md', body: readme },
      { name: prefix + '/canonic-export.json', body: baseReport + ' '.repeat(reportReserve) },
      { name: prefix + '/canonic-export-part-9999.json', body: listed },
      ...group,
    ]);
  }

  payload.forEach(function (entry) {
    var candidate = current.concat([entry]);
    if (current.length && provisional(candidate) > maximum) {
      groups.push(current);
      current = [entry];
    } else current = candidate;
    if (provisional(current) > maximum) {
      throw new Error('“' + entry.name.slice(prefix.length + 1) + '” cannot fit inside the 10 MB export ZIP limit');
    }
  });
  if (current.length || !groups.length) groups.push(current);

  var archives: Archive[];
  while (true) {
    var total = groups.length;
    var width = Math.max(2, String(total).length);
    function filename(index: number) {
      return total === 1 ? prefix + '.zip' : prefix + '-part-' + String(index + 1).padStart(width, '0') + '-of-' + String(total).padStart(width, '0') + '.zip';
    }
    report.parts = groups.map(function (group, index) {
      return { number: index + 1, filename: filename(index), files: group.length };
    });
    var reportBody = JSON.stringify(report, null, 2) + '\n';
    var splitReadme = readme + '\n## Archive parts\n\n' +
      (total === 1
        ? 'This export fits in one ZIP file.\n'
        : 'This export is split into ' + total + ' ZIP files so every upload remains at or below 10 MB. Extract every part into the same directory; their shared top-level folder and repeated metadata are designed to merge.\n');
    archives = groups.map(function (group, index) {
      var part = {
        part: index + 1, of: total, filename: filename(index),
        files: group.map(function (entry) { return entry.name.slice(prefix.length + 1); }),
      };
      var common: ZipEntry[] = [
        { name: prefix + '/README.md', body: splitReadme },
        { name: prefix + '/canonic-export.json', body: reportBody },
        { name: prefix + '/canonic-export-part-' + String(index + 1).padStart(width, '0') + '.json', body: JSON.stringify(part, null, 2) + '\n' },
      ];
      var body = zip(common.concat(group));
      return { filename: filename(index), body: body, files: part.files };
    });
    var oversized = archives.findIndex(function (archive) { return archive.body.length > maximum; });
    if (oversized < 0) break;
    if (groups[oversized].length <= 1) {
      throw new Error('“' + groups[oversized][0].name.slice(prefix.length + 1) + '” cannot fit inside the 10 MB export ZIP limit');
    }
    var moved = groups[oversized].pop()!;
    if (groups[oversized + 1]) groups[oversized + 1].unshift(moved);
    else groups.push([moved]);
  }
  return archives;
}
