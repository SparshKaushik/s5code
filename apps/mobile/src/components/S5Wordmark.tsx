import type { ColorValue } from "react-native";
import { Text } from "react-native";

export function S5Wordmark(props: { readonly height: number; readonly color: ColorValue }) {
  return (
    <Text
      accessibilityLabel="S5"
      style={{
        color: props.color,
        fontFamily: "DMSans-Bold",
        fontSize: props.height,
        letterSpacing: -0.6,
        lineHeight: props.height,
      }}
    >
      S5
    </Text>
  );
}
