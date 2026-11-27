// Sharing programs through the URL: #code=<base64url of the deflated source>.

const PREFIX = 'code=';

function toBase64Url(bytes) {
  let binary = '';
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

function fromBase64Url(text) {
  const binary = atob(text.replace(/-/g, '+').replace(/_/g, '/'));
  return Uint8Array.from(binary, (char) => char.charCodeAt(0));
}

async function pipe(bytes, transform) {
  const stream = new Blob([bytes]).stream().pipeThrough(transform);
  return new Uint8Array(await new Response(stream).arrayBuffer());
}

/** The URL hash that encodes `source`. */
export async function encodeShare(source) {
  const bytes = new TextEncoder().encode(source);
  return `#${PREFIX}${toBase64Url(await pipe(bytes, new CompressionStream('deflate-raw')))}`;
}

/** The source encoded in a URL hash, or null. */
export async function decodeShare(hash) {
  if (!hash.startsWith(`#${PREFIX}`)) return null;
  try {
    const bytes = await pipe(fromBase64Url(hash.slice(PREFIX.length + 1)), new DecompressionStream('deflate-raw'));
    return new TextDecoder().decode(bytes);
  } catch {
    return null;
  }
}
