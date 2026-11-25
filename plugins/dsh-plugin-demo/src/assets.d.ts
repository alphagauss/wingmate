declare module '*.css' {
  const css: string;
  export default css;
}
declare module '@deepseek-ai/dsh-client-ui-primitives' {
  export const Button: import('./ui/controls.tsx').ButtonComponent;
}
