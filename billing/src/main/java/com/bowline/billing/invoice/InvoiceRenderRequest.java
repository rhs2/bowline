package com.bowline.billing.invoice;

import com.bowline.billing.document.PostalAddress;
import jakarta.validation.Valid;
import jakarta.validation.constraints.DecimalMax;
import jakarta.validation.constraints.DecimalMin;
import jakarta.validation.constraints.Digits;
import jakarta.validation.constraints.Min;
import jakarta.validation.constraints.NotBlank;
import jakarta.validation.constraints.NotEmpty;
import jakarta.validation.constraints.NotNull;
import jakarta.validation.constraints.Pattern;
import jakarta.validation.constraints.Size;
import java.math.BigDecimal;
import java.time.LocalDate;
import java.util.List;

/**
 * Body of {@code POST /render/invoice}: the invoice header, the customer it is billed
 * to, an optional shipment reference and the lines. Keys are snake_case on the wire
 * ({@code invoice_no}, {@code unit_price}); money is a decimal string or number.
 */
public record InvoiceRenderRequest(
        @NotNull @Valid Invoice invoice,
        @NotNull @Valid Customer customer,
        @Valid Shipment shipment,
        @NotEmpty @Size(max = 500) List<@Valid Line> lines) {

    /** Header fields, mirroring the {@code invoices} table. */
    public record Invoice(
            @NotBlank
            @Pattern(regexp = "^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$", message = "must be a reference such as INV-2026-000123")
            String invoiceNo,
            @NotNull LocalDate issueDate,
            @NotNull LocalDate dueDate,
            @NotBlank @Pattern(regexp = "^[A-Z]{3}$", message = "must be a three-letter currency code") String currency,
            @NotNull @DecimalMin("0.00") @Digits(integer = 12, fraction = 2) BigDecimal subtotal,
            @NotNull @DecimalMin("0.00") @Digits(integer = 12, fraction = 2) BigDecimal tax,
            @NotNull @DecimalMin("0.00") @Digits(integer = 12, fraction = 2) BigDecimal total,
            @NotNull @DecimalMin("0.00") @Digits(integer = 12, fraction = 2) BigDecimal amountPaid,
            @Size(max = 2000) String notes) {

        /** {@code tax} and {@code amount_paid} default to zero when omitted. */
        public Invoice {
            tax = tax == null ? BigDecimal.ZERO : tax;
            amountPaid = amountPaid == null ? BigDecimal.ZERO : amountPaid;
        }

        public BigDecimal balanceDue() {
            return total.subtract(amountPaid);
        }
    }

    /** The bill-to party, from the {@code customers} table. */
    public record Customer(
            @NotBlank @Size(max = 200) String name,
            @Size(max = 40) String code,
            @Valid Contact contact,
            @Valid PostalAddress billingAddress) {}

    /** Contact person for the customer; every field optional. */
    public record Contact(
            @Size(max = 200) String name,
            @Size(max = 200) String email,
            @Size(max = 50) String phone) {}

    /** Optional shipment the invoice bills. */
    public record Shipment(
            @Size(max = 40) String reference,
            @Size(max = 20) String mode,
            @Size(max = 200) String origin,
            @Size(max = 200) String destination) {}

    /** One invoice line, mirroring the {@code invoice_lines} table. */
    public record Line(
            @NotNull @Min(1) Integer seq,
            @NotBlank @Size(max = 500) String description,
            @NotNull @DecimalMin(value = "0", inclusive = false) @Digits(integer = 9, fraction = 3) BigDecimal quantity,
            @NotNull @DecimalMin("0.00") @Digits(integer = 12, fraction = 2) BigDecimal unitPrice,
            @NotNull @DecimalMin("0") @DecimalMax("1") @Digits(integer = 1, fraction = 4) BigDecimal taxRate,
            @NotNull @DecimalMin("0.00") @Digits(integer = 12, fraction = 2) BigDecimal amount) {

        /** {@code tax_rate} defaults to zero when omitted. */
        public Line {
            taxRate = taxRate == null ? BigDecimal.ZERO : taxRate;
        }
    }
}
