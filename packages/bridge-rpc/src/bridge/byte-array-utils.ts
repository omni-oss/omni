export async function readAll(
    gen: AsyncIterable<Uint8Array>,
): Promise<Uint8Array> {
    const chunks: Uint8Array[] = [];
    for await (const chunk of gen) {
        chunks.push(chunk);
    }
    const wholeChunk = new Uint8Array(
        chunks.reduce((acc, c) => acc + c.length, 0),
    );

    for (const chunk of chunks) {
        wholeChunk.set(chunk, wholeChunk.length - chunk.length);
    }

    return wholeChunk;
}
