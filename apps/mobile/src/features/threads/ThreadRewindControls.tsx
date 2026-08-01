import type { MenuAction } from "@react-native-menu/menu";
import { useMemo, type ReactElement } from "react";

import { AndroidHeaderIconButton } from "../../components/AndroidScreenHeader";
import { ControlPillMenu } from "../../components/ControlPill";
import { NativeHeaderToolbar } from "../../native/StackHeader";
import {
  REWIND_MENU_ACCESSIBILITY_LABEL,
  REWIND_MENU_TITLE,
  resolveRewindMenuDirection,
  type RewindMenuRow,
  type ThreadRewindMenuModel,
} from "./threadRewindPresentation";

/**
 * Native iOS header item for the rewind menu, or `null` when the server
 * reports rewind unavailable (experiment off, older server, or a thread with
 * no snapshottable workspace) so no dead control appears.
 */
export function rewindHeaderItem(
  model: ThreadRewindMenuModel | undefined,
): Record<string, unknown> | null {
  if (!model?.available) {
    return null;
  }
  return {
    accessibilityLabel: REWIND_MENU_ACCESSIBILITY_LABEL,
    icon: { name: "arrow.uturn.backward", type: "sfSymbol" },
    identifier: "thread-right-rewind",
    label: "Rewind",
    menu: {
      items: model.rows.map((row) => ({
        description: row.detail,
        disabled: row.disabled,
        icon: { name: row.icon, type: "sfSymbol" as const },
        label: row.label,
        onPress: () => model.onStep(row.direction),
        type: "action" as const,
      })),
      title: REWIND_MENU_TITLE,
    },
    sharesBackground: true,
    type: "menu",
    variant: "plain",
  };
}

function toAndroidMenuAction(row: RewindMenuRow): MenuAction {
  return {
    id: row.id,
    title: row.label,
    subtitle: row.detail,
    image: row.icon,
    attributes: row.disabled ? { disabled: true } : undefined,
  };
}

/**
 * Rewind menu for the in-content `NativeHeaderToolbar` (iOS versions without
 * native glass headers).
 *
 * A plain function rather than a component: the toolbar converts its DIRECT
 * children by display name, so a wrapper component's element would be dropped.
 */
export function renderRewindToolbarMenu(
  model: ThreadRewindMenuModel | undefined,
): ReactElement | null {
  if (!model?.available) {
    return null;
  }
  return (
    <NativeHeaderToolbar.Menu
      accessibilityLabel={REWIND_MENU_ACCESSIBILITY_LABEL}
      icon="arrow.uturn.backward"
      title={REWIND_MENU_TITLE}
    >
      {model.rows.map((row) => (
        <NativeHeaderToolbar.MenuAction
          key={row.id}
          disabled={row.disabled}
          icon={row.icon}
          onPress={() => model.onStep(row.direction)}
          subtitle={row.detail}
        >
          <NativeHeaderToolbar.Label>{row.label}</NativeHeaderToolbar.Label>
        </NativeHeaderToolbar.MenuAction>
      ))}
    </NativeHeaderToolbar.Menu>
  );
}

/**
 * Android in-flow header counterpart. Android draws its own header, so the
 * rewind menu is an icon button anchored to the token-styled dropdown instead
 * of a native bar-button menu.
 */
export function AndroidThreadRewindMenu(props: {
  readonly model: ThreadRewindMenuModel | undefined;
}) {
  const model = props.model;
  const actions = useMemo(
    () => (model?.available ? model.rows.map(toAndroidMenuAction) : []),
    [model?.available, model?.rows],
  );

  if (!model?.available) {
    return null;
  }

  return (
    <ControlPillMenu
      actions={actions}
      isAnchoredToRight
      title={REWIND_MENU_TITLE}
      onPressAction={(event) => {
        const direction = resolveRewindMenuDirection(event.nativeEvent.event);
        if (direction) {
          model.onStep(direction);
        }
      }}
    >
      <AndroidHeaderIconButton
        accessibilityLabel={REWIND_MENU_ACCESSIBILITY_LABEL}
        icon="arrow.uturn.backward"
      />
    </ControlPillMenu>
  );
}
