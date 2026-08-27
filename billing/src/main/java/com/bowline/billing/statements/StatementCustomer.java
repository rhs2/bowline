package com.bowline.billing.statements;

import com.bowline.billing.document.PostalAddress;
import java.util.UUID;

/** The customer a statement is addressed to (from the {@code customers} table). */
public record StatementCustomer(
        UUID id,
        String code,
        String name,
        String contactName,
        String contactEmail,
        PostalAddress billingAddress,
        String currency) {}
