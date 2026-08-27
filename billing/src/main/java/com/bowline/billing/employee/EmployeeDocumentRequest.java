package com.bowline.billing.employee;

import jakarta.validation.Valid;
import jakarta.validation.constraints.DecimalMin;
import jakarta.validation.constraints.Digits;
import jakarta.validation.constraints.Max;
import jakarta.validation.constraints.Min;
import jakarta.validation.constraints.NotBlank;
import jakarta.validation.constraints.NotNull;
import jakarta.validation.constraints.Pattern;
import jakarta.validation.constraints.Size;
import java.math.BigDecimal;
import java.time.LocalDate;

/**
 * Body of {@code POST /render/document}: which personnel file to draw, who it belongs
 * to, the object key it must be stored under, and one detail block per kind. Keys are
 * snake_case on the wire ({@code s3_key}, {@code employee.employee_no}); only the block
 * matching {@code kind} is read, the others may be omitted.
 *
 * <p>The caller owns the object key, because the row in {@code employee_documents}
 * already names it. The pattern keeps a key inside the employee document prefix, so a
 * render can never write over an invoice or escape the bucket layout.
 */
public record EmployeeDocumentRequest(
        @NotBlank
        @Pattern(regexp = "contract|payslip|certificate|id",
                message = "must be one of contract, payslip, certificate, id")
        String kind,

        @NotBlank
        @Size(max = 400)
        @Pattern(regexp = KEY_PATTERN,
                message = "must be an employee document key such as employees/<employee id>/contract.pdf")
        String s3Key,

        @NotBlank @Size(max = 200) String title,
        @NotNull @Valid Employee employee,
        @Valid Contract contract,
        @Valid Payslip payslip,
        @Valid Certificate certificate,
        @Valid Identity identity) {

    /**
     * Segments must start with a letter or a digit, which is what keeps {@code .} and
     * {@code ..} out of the key, and the object must be a PDF.
     */
    static final String KEY_PATTERN =
            "^employees/(?:[A-Za-z0-9][A-Za-z0-9._-]*/)+[A-Za-z0-9][A-Za-z0-9._-]*\\.pdf$";

    /** Who the file belongs to, from {@code employees} and its position and department. */
    public record Employee(
            @NotBlank @Size(max = 40) String employeeNo,
            @NotBlank @Size(max = 200) String name,
            @Size(max = 200) String email,
            @Size(max = 200) String positionTitle,
            @Size(max = 200) String department,
            @Size(max = 200) String site,
            @Size(max = 40) String payGrade,
            @Size(max = 200) String managerName,
            LocalDate hireDate,
            @Size(max = 40) String employmentType) {}

    /** {@code kind=contract}: the terms of employment. */
    public record Contract(
            @NotBlank @Size(max = 200) String title,
            @Size(max = 200) String department,
            @NotNull LocalDate startDate,
            @NotNull @DecimalMin("0.00") @Digits(integer = 10, fraction = 2) BigDecimal salary,
            @Pattern(regexp = "^[A-Z]{3}$", message = "must be a three-letter currency code") String currency,
            @NotBlank @Size(max = 40) String employmentType,
            @Size(max = 40) String payGrade,
            @Size(max = 200) String site,
            @Min(1) @Max(80) Integer weeklyHours,
            @Min(0) @Max(365) Integer noticeDays) {}

    /** {@code kind=payslip}: one pay period. */
    public record Payslip(
            @NotBlank @Pattern(regexp = "^\\d{4}-\\d{2}$", message = "must be a month such as 2026-07") String period,
            LocalDate periodStart,
            LocalDate periodEnd,
            LocalDate payDate,
            @NotNull @DecimalMin("0.00") @Digits(integer = 10, fraction = 2) BigDecimal gross,
            @NotNull @DecimalMin("0.00") @Digits(integer = 10, fraction = 2) BigDecimal deductions,
            @NotNull @DecimalMin("0.00") @Digits(integer = 10, fraction = 2) BigDecimal net,
            @Pattern(regexp = "^[A-Z]{3}$", message = "must be a three-letter currency code") String currency,
            @Size(max = 40) String payMethod) {}

    /** {@code kind=certificate}: a qualification held by the employee. */
    public record Certificate(
            @NotBlank @Size(max = 200) String name,
            @Size(max = 200) String issuer,
            @NotNull LocalDate issuedOn,
            LocalDate expiresOn,
            @Size(max = 80) String reference) {}

    /** {@code kind=id}: the identity document HR holds on file. */
    public record Identity(
            @NotBlank @Size(max = 120) String documentType,
            @Size(max = 80) String number,
            @Size(max = 120) String issuingAuthority,
            LocalDate issuedOn,
            LocalDate expiresOn) {}
}
