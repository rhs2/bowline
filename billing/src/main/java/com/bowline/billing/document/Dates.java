package com.bowline.billing.document;

import java.time.LocalDate;
import java.time.format.DateTimeFormatter;
import java.util.Locale;

/** Date formatting for documents: {@code 27 Aug 2026}. */
public final class Dates {

    private static final DateTimeFormatter LONG = DateTimeFormatter.ofPattern("d MMM uuuu", Locale.ENGLISH);

    private Dates() {}

    public static String format(LocalDate date) {
        return date == null ? "" : LONG.format(date);
    }
}
