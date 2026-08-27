package com.bowline.billing.employee;

import java.util.Locale;
import java.util.Optional;

/**
 * The personnel file kinds the service can render. They are a subset of the
 * {@code employee_documents.kind} values: {@code other} covers whatever HR chose to
 * upload and has no layout of its own.
 */
public enum EmployeeDocumentKind {
    CONTRACT,
    PAYSLIP,
    CERTIFICATE,
    ID;

    /** The value used on the wire and in {@code employee_documents.kind}. */
    public String wireName() {
        return name().toLowerCase(Locale.ROOT);
    }

    /** The kinds this service renders, as a comma separated list for error messages. */
    public static String allWireNames() {
        StringBuilder sb = new StringBuilder();
        for (EmployeeDocumentKind kind : values()) {
            if (sb.length() > 0) {
                sb.append(", ");
            }
            sb.append(kind.wireName());
        }
        return sb.toString();
    }

    public static Optional<EmployeeDocumentKind> of(String value) {
        if (value == null) {
            return Optional.empty();
        }
        for (EmployeeDocumentKind kind : values()) {
            if (kind.wireName().equals(value.trim().toLowerCase(Locale.ROOT))) {
                return Optional.of(kind);
            }
        }
        return Optional.empty();
    }
}
