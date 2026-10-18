/** The file's own name: what follows the last slash or backslash of its path. */
export const fileName = (path: string): string => path.split(/[\\/]/).pop() ?? path;
