import { deflateRawSync } from "node:zlib";

const CRC_TABLE = (() => {
    const table = new Int32Array(256);
    for (let n = 0; n < 256; n++) {
        let c = n;
        for (let k = 0; k < 8; k++) {
            c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
        }
        table[n] = c;
    }
    return table;
})();

function crc32(buffer) {
    let crc = -1;
    for (let i = 0; i < buffer.length; i++) {
        crc = (crc >>> 8) ^ CRC_TABLE[(crc ^ buffer[i]) & 0xff];
    }
    return (crc ^ -1) >>> 0;
}

function dosDateTime(date) {
    const time = (date.getHours() << 11) | (date.getMinutes() << 5) | (date.getSeconds() >> 1);
    const day = ((date.getFullYear() - 1980) << 9) | ((date.getMonth() + 1) << 5) | date.getDate();
    return { time, day };
}

export function createZip(entries) {
    const chunks = [];
    const central = [];
    let offset = 0;
    const now = new Date();
    const { time, day } = dosDateTime(now);

    for (const entry of entries) {
        const name = Buffer.from(entry.name, "utf8");
        const data = Buffer.isBuffer(entry.data) ? entry.data : Buffer.from(entry.data);
        const deflated = deflateRawSync(data);
        const useDeflate = deflated.length < data.length;
        const payload = useDeflate ? deflated : data;
        const method = useDeflate ? 8 : 0;
        const checksum = crc32(data);

        const local = Buffer.alloc(30);
        local.writeUInt32LE(0x04034b50, 0);
        local.writeUInt16LE(20, 4);
        local.writeUInt16LE(0x0800, 6);
        local.writeUInt16LE(method, 8);
        local.writeUInt16LE(time, 10);
        local.writeUInt16LE(day, 12);
        local.writeUInt32LE(checksum, 14);
        local.writeUInt32LE(payload.length, 18);
        local.writeUInt32LE(data.length, 22);
        local.writeUInt16LE(name.length, 26);
        local.writeUInt16LE(0, 28);

        chunks.push(local, name, payload);

        const header = Buffer.alloc(46);
        header.writeUInt32LE(0x02014b50, 0);
        header.writeUInt16LE(20, 4);
        header.writeUInt16LE(20, 6);
        header.writeUInt16LE(0x0800, 8);
        header.writeUInt16LE(method, 10);
        header.writeUInt16LE(time, 12);
        header.writeUInt16LE(day, 14);
        header.writeUInt32LE(checksum, 16);
        header.writeUInt32LE(payload.length, 20);
        header.writeUInt32LE(data.length, 24);
        header.writeUInt16LE(name.length, 28);
        header.writeUInt32LE(offset, 42);
        central.push(header, name);

        offset += local.length + name.length + payload.length;
    }

    const centralSize = central.reduce((sum, part) => sum + part.length, 0);
    const end = Buffer.alloc(22);
    end.writeUInt32LE(0x06054b50, 0);
    end.writeUInt16LE(entries.length, 8);
    end.writeUInt16LE(entries.length, 10);
    end.writeUInt32LE(centralSize, 12);
    end.writeUInt32LE(offset, 16);

    return Buffer.concat([...chunks, ...central, end]);
}
