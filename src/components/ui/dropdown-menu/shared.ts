import { type InjectionKey, type Ref } from "vue";

/** Injection key for the dropdown open state (boolean ref). */
export const DROPDOWN_OPEN_KEY: InjectionKey<Ref<boolean>> = Symbol("dropdown-open");

/** Injection key for the dropdown close callback. */
export const DROPDOWN_CLOSE_KEY: InjectionKey<() => void> = Symbol("dropdown-close");
