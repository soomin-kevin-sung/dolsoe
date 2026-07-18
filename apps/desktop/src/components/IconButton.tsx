import type { ButtonHTMLAttributes, ComponentType, Ref } from "react";
import type { LucideProps } from "lucide-react";

interface IconButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  icon: ComponentType<LucideProps>;
  label: string;
  buttonRef?: Ref<HTMLButtonElement>;
}

export function IconButton({ icon: Icon, label, className = "", buttonRef, ...props }: IconButtonProps) {
  return (
    <button ref={buttonRef} type="button" className={`icon-button ${className}`.trim()} aria-label={label} title={label} {...props}>
      <Icon size={16} strokeWidth={2} aria-hidden="true" />
    </button>
  );
}
