// Asset imports of the bundlers (Vite, Astro) that the shared code uses.
declare module '*.css';
declare module '*?raw' {
  const text: string;
  export default text;
}
// Vite's `import.meta.glob`, only in the form the shared code uses: eager text of every matching file, keyed by path.
interface ImportMeta {
  glob(pattern: string, options: { query: '?raw'; import: 'default'; eager: true }): Record<string, string>;
}
