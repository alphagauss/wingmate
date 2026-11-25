import React from 'react';
import type { ButtonHTMLAttributes, ComponentType, ReactNode } from 'react';

export type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: 'primary' | 'outline' | 'ghost';
  size?: 'md' | 'sm';
  icon?: ReactNode;
};
export type ButtonComponent = ComponentType<ButtonProps>;

// The standalone page uses the same control dimensions and theme tokens as DSH.
export function WebButton({
  variant = 'ghost',
  size = 'md',
  icon,
  className = '',
  children,
  ...props
}: ButtonProps) {
  return (
    <button
      type="button"
      className={`plugin-demo-button ${className}`}
      data-variant={variant}
      data-size={size}
      {...props}
    >
      {icon}
      {children}
    </button>
  );
}

export function StatisticsIcon() {
  return (
    <svg
      viewBox="0 0 24 24"
      width="20"
      height="20"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.6"
      strokeLinecap="round"
      aria-hidden="true"
    >
      <path d="M4 5h16M4 10h12M4 15h7M16 14v6M21 10v10M11 18v2" />
    </svg>
  );
}
