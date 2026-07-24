import { useEffect, useRef, type ReactNode } from "react";
import { AppWindow, Cpu, Gauge, SlidersHorizontal, UserRoundCog, X, type LucideIcon } from "lucide-react";

import { IconButton } from "./IconButton";

export type SettingsTab = "general" | "persona" | "generation" | "performance" | "runtime";

const tabs: { value: SettingsTab; label: string; icon: LucideIcon }[] = [
  { value: "general", label: "일반", icon: AppWindow },
  { value: "persona", label: "페르소나", icon: UserRoundCog },
  { value: "generation", label: "생성", icon: SlidersHorizontal },
  { value: "performance", label: "성능", icon: Gauge },
  { value: "runtime", label: "런타임", icon: Cpu },
];

interface Props {
  open: boolean;
  activeTab: SettingsTab;
  closeOnEscape?: boolean;
  children: ReactNode;
  footer?: ReactNode;
  availableTabs?: SettingsTab[];
  onTabChange(tab: SettingsTab): void;
  onClose(): void;
}

const defaultTabs: SettingsTab[] = ["general", "generation", "performance", "runtime"];

export function SettingsDialog({ open, activeTab, closeOnEscape = true, children, footer, availableTabs = defaultTabs, onTabChange, onClose }: Props) {
  const closeButtonRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLElement>(null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  useEffect(() => {
    if (!open) return;

    const previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const focusFrame = requestAnimationFrame(() => closeButtonRef.current?.focus());
    return () => {
      cancelAnimationFrame(focusFrame);
      previousFocus?.focus();
    };
  }, [open]);

  useEffect(() => {
    if (!open) return;

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && closeOnEscape) {
        const dialogs = document.querySelectorAll<HTMLElement>('[role="dialog"][aria-modal="true"]');
        if (dialogs.item(dialogs.length - 1) !== panelRef.current) return;
        event.preventDefault();
        onCloseRef.current();
      }
    };

    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [closeOnEscape, open]);

  if (!open) return null;

  return (
    <div className="settings-scrim" onMouseDown={(event) => {
      if (event.target === event.currentTarget && closeOnEscape) onClose();
    }}>
      <section ref={panelRef} className="settings-panel" role="dialog" aria-modal="true" aria-labelledby="settings-dialog-title">
        <div className="panel-header">
          <h2 id="settings-dialog-title">설정</h2>
          <IconButton buttonRef={closeButtonRef} icon={X} label="설정 닫기" onClick={onClose} />
        </div>
        <div className="settings-main">
          <div className="settings-tabs" role="tablist" aria-label="설정 분류">
            {tabs.filter((tab) => availableTabs.includes(tab.value)).map((tab) => {
              const Icon = tab.icon;
              return <button
                key={tab.value}
                type="button"
                role="tab"
                aria-selected={activeTab === tab.value}
                aria-controls={`settings-panel-${tab.value}`}
                className={activeTab === tab.value ? "selected" : ""}
                onClick={() => onTabChange(tab.value)}
              >
                <Icon size={16} strokeWidth={2} aria-hidden="true" />
                <span>{tab.label}</span>
              </button>;
            })}
          </div>
          <div className="panel-body">{children}</div>
        </div>
        {footer && <div className="panel-footer">{footer}</div>}
      </section>
    </div>
  );
}
