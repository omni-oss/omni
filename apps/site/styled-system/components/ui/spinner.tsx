import { ark } from "@ark-ui/solid/factory";
import type { ComponentProps } from "solid-js";
import { styled } from "~/jsx";
import { spinner } from "~/recipes";

export type SpinnerProps = ComponentProps<typeof Spinner>;
export const Spinner = styled(ark.span, spinner);
