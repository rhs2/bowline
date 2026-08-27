package com.bowline.billing.web;

import java.util.Locale;
import java.util.Set;

/** Query flags are written {@code ?flag=1}; {@code true}, {@code yes} and {@code on} mean the same. */
public final class Flags {

    private static final Set<String> TRUTHY = Set.of("1", "true", "yes", "on");

    private Flags() {}

    public static boolean truthy(String value) {
        return value != null && TRUTHY.contains(value.trim().toLowerCase(Locale.ROOT));
    }
}
