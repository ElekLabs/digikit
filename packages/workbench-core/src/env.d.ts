// Asset imports of the bundlers (Vite, Astro) that the shared code uses.
declare module '*.css';
declare module '*?raw' {
  const text: string;
  export default text;
}
