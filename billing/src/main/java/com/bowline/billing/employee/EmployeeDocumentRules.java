package com.bowline.billing.employee;

import com.bowline.billing.web.InvalidRequestException;
import com.bowline.billing.web.ProblemResponse;
import java.math.BigDecimal;
import java.time.LocalDate;
import java.util.ArrayList;
import java.util.List;

/**
 * Cross-field rules bean validation cannot express: the detail block has to match the
 * kind, and the figures and dates inside it have to be consistent. A personnel file
 * with the wrong numbers on it is worse than no file, so every one of these is a hard
 * 422 rather than a warning.
 */
final class EmployeeDocumentRules {

    private EmployeeDocumentRules() {}

    static EmployeeDocumentKind check(EmployeeDocumentRequest request) {
        EmployeeDocumentKind kind = EmployeeDocumentKind.of(request.kind())
                .orElseThrow(() -> new InvalidRequestException(
                        "kind", "must be one of " + EmployeeDocumentKind.allWireNames()));
        List<ProblemResponse.FieldError> errors = new ArrayList<>();
        switch (kind) {
            case CONTRACT -> checkContract(request.contract(), errors);
            case PAYSLIP -> checkPayslip(request.payslip(), errors);
            case CERTIFICATE -> checkCertificate(request.certificate(), errors);
            case ID -> checkIdentity(request.identity(), errors);
        }
        if (!errors.isEmpty()) {
            throw new InvalidRequestException("The document details do not match the requested kind.", errors);
        }
        return kind;
    }

    private static void checkContract(EmployeeDocumentRequest.Contract contract, List<ProblemResponse.FieldError> errors) {
        if (required(contract, "contract", "contract", errors)) {
            return;
        }
        if (contract.startDate().isBefore(LocalDate.of(1900, 1, 1))) {
            errors.add(new ProblemResponse.FieldError("contract.start_date", "must be a plausible date"));
        }
    }

    private static void checkPayslip(EmployeeDocumentRequest.Payslip payslip, List<ProblemResponse.FieldError> errors) {
        if (required(payslip, "payslip", "payslip", errors)) {
            return;
        }
        if (payslip.gross().subtract(payslip.deductions()).compareTo(payslip.net()) != 0) {
            errors.add(new ProblemResponse.FieldError("payslip.net", "must equal gross - deductions"));
        }
        if (payslip.deductions().compareTo(payslip.gross()) > 0) {
            errors.add(new ProblemResponse.FieldError("payslip.deductions", "must not exceed gross"));
        }
        if (payslip.gross().compareTo(BigDecimal.ZERO) == 0) {
            errors.add(new ProblemResponse.FieldError("payslip.gross", "must be greater than zero"));
        }
        order(payslip.periodStart(), payslip.periodEnd(), "payslip.period_end", "period_start", errors);
    }

    private static void checkCertificate(
            EmployeeDocumentRequest.Certificate certificate, List<ProblemResponse.FieldError> errors) {
        if (required(certificate, "certificate", "certificate", errors)) {
            return;
        }
        order(certificate.issuedOn(), certificate.expiresOn(), "certificate.expires_on", "issued_on", errors);
    }

    private static void checkIdentity(EmployeeDocumentRequest.Identity identity, List<ProblemResponse.FieldError> errors) {
        if (required(identity, "identity", "id", errors)) {
            return;
        }
        order(identity.issuedOn(), identity.expiresOn(), "identity.expires_on", "issued_on", errors);
    }

    /** True when the block is missing, in which case no further check can run. */
    private static boolean required(Object block, String field, String kind, List<ProblemResponse.FieldError> errors) {
        if (block == null) {
            errors.add(new ProblemResponse.FieldError(field, "is required for kind " + kind));
            return true;
        }
        return false;
    }

    private static void order(
            LocalDate from, LocalDate to, String field, String other, List<ProblemResponse.FieldError> errors) {
        if (from != null && to != null && to.isBefore(from)) {
            errors.add(new ProblemResponse.FieldError(field, "must be on or after " + other));
        }
    }
}
