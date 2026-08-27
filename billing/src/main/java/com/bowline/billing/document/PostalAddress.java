package com.bowline.billing.document;

import jakarta.validation.constraints.Size;
import java.util.ArrayList;
import java.util.List;

/**
 * The shape stored in {@code customers.billing_address} (jsonb) and accepted in render
 * requests. Every field is optional; unknown keys are ignored on input.
 */
public record PostalAddress(
        @Size(max = 200) String line1,
        @Size(max = 200) String line2,
        @Size(max = 100) String city,
        @Size(max = 100) String region,
        @Size(max = 20) String postalCode,
        @Size(max = 100) String country) {

    /** Non-blank address lines in postal order. */
    public List<String> lines() {
        List<String> out = new ArrayList<>(4);
        add(out, line1);
        add(out, line2);
        String locality = join(" ", join(", ", city, region), postalCode);
        add(out, locality);
        add(out, country);
        return out;
    }

    private static void add(List<String> out, String value) {
        if (value != null && !value.isBlank()) {
            out.add(value.trim());
        }
    }

    private static String join(String separator, String a, String b) {
        boolean hasA = a != null && !a.isBlank();
        boolean hasB = b != null && !b.isBlank();
        if (hasA && hasB) {
            return a.trim() + separator + b.trim();
        }
        return hasA ? a.trim() : hasB ? b.trim() : "";
    }
}
