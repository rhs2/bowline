package com.bowline.billing.document;

import java.math.BigDecimal;
import java.math.RoundingMode;
import java.util.Locale;

/** Formatting helpers for amounts shown on documents. Thread safe, allocation light. */
public final class Money {

    private Money() {}

    /** {@code 1234567.5 -> "1,234,567.50"}. */
    public static String amount(BigDecimal value) {
        BigDecimal v = value == null ? BigDecimal.ZERO : value;
        return String.format(Locale.US, "%,.2f", v.setScale(2, RoundingMode.HALF_UP));
    }

    /** {@code "USD 1,250.00"}. */
    public static String amount(BigDecimal value, String currency) {
        return currency + " " + amount(value);
    }

    /** Quantities keep only the decimals they need: {@code 2.000 -> "2"}, {@code 1.250 -> "1.25"}. */
    public static String quantity(BigDecimal value) {
        BigDecimal v = value == null ? BigDecimal.ZERO : value.stripTrailingZeros();
        int scale = Math.max(0, Math.min(3, v.scale()));
        return String.format(Locale.US, "%,." + scale + "f", v.setScale(scale, RoundingMode.HALF_UP));
    }

    /** A rate between 0 and 1 as a percentage: {@code 0.0825 -> "8.25%"}. */
    public static String percent(BigDecimal rate) {
        BigDecimal v = rate == null ? BigDecimal.ZERO : rate;
        BigDecimal pct = v.multiply(BigDecimal.valueOf(100)).setScale(2, RoundingMode.HALF_UP).stripTrailingZeros();
        if (pct.scale() < 0) {
            pct = pct.setScale(0);
        }
        return pct.toPlainString() + "%";
    }

    public static BigDecimal zeroIfNull(BigDecimal value) {
        return value == null ? BigDecimal.ZERO : value;
    }
}
