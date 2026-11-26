declare module '*.css' {
  const css: string;
  export default css;
}
declare module '@deepseek-ai/dsh-client-ui-primitives' {
  import type {
    ButtonHTMLAttributes,
    ComponentType,
    InputHTMLAttributes,
    ReactNode,
  } from 'react';
  export const Button: ComponentType<
    ButtonHTMLAttributes<HTMLButtonElement> & {
      variant?: 'primary' | 'outline' | 'ghost';
      size?: 'md' | 'sm';
      icon?: ReactNode;
    }
  >;
  export const Input: ComponentType<
    InputHTMLAttributes<HTMLInputElement> & { icon?: ReactNode }
  >;
  export const MenuItemButton: ComponentType<{
    icon?: ReactNode;
    onSelect(): void;
    children: ReactNode;
  }>;
  export const StateDot: ComponentType<{ state: 'ongoing' }>;
  export const Tag: ComponentType<{
    tone?: 'neutral' | 'success' | 'warning';
    children: ReactNode;
  }>;
  export const Modal: ComponentType<{
    open: boolean;
    onClose(): void;
    title: string;
    closeLabel: string;
    description?: string;
    className?: string;
    contentClassName?: string;
    children: ReactNode;
  }>;
}
