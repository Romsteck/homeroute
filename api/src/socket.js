// Singleton pour l'instance Socket.IO
let io = null;

export function setIO(ioInstance) {
  io = ioInstance;
}

export function getIO() {
  return io;
}
