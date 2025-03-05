
export { init } from "./init"
export * from "./constants"
export * from "./pdas"
export * from "./transaction"

export function nextAvailableTaskIds(taskBitmap: Buffer, n: number): number[] {
  if (n === 0) {
    return [];
  }

  const availableTaskIds: number[] = [];
  const randStart = Math.floor(Math.random() * taskBitmap.length);
  for (let byteOffset = 0; byteOffset < taskBitmap.length; byteOffset++) {
    const byteIdx = (byteOffset + randStart) % taskBitmap.length;
    const byte = taskBitmap[byteIdx];
    if (byte !== 0xff) {
      // If byte is not all 1s
      for (let bitIdx = 0; bitIdx < 8; bitIdx++) {
        if ((byte & (1 << bitIdx)) === 0) {
          availableTaskIds.push(byteIdx * 8 + bitIdx);
          if (availableTaskIds.length === n) {
            return availableTaskIds;
          }
        }
      }
    }
  }
  return availableTaskIds;
}
