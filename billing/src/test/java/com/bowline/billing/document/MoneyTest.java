package com.bowline.billing.document;

import static org.assertj.core.api.Assertions.assertThat;

import java.math.BigDecimal;
import java.util.List;
import org.junit.jupiter.api.Test;

class MoneyTest {

    @Test
    void amountsUseTwoDecimalsAndGrouping() {
        assertThat(Money.amount(new BigDecimal("1234567.5"))).isEqualTo("1,234,567.50");
        assertThat(Money.amount(new BigDecimal("0"))).isEqualTo("0.00");
        assertThat(Money.amount(new BigDecimal("-1000"))).isEqualTo("-1,000.00");
        assertThat(Money.amount(new BigDecimal("2.345"))).isEqualTo("2.35");
        assertThat(Money.amount(new BigDecimal("99.99"), "EUR")).isEqualTo("EUR 99.99");
        assertThat(Money.amount(null)).isEqualTo("0.00");
    }

    @Test
    void quantitiesDropUnneededDecimals() {
        assertThat(Money.quantity(new BigDecimal("2.000"))).isEqualTo("2");
        assertThat(Money.quantity(new BigDecimal("12.500"))).isEqualTo("12.5");
        assertThat(Money.quantity(new BigDecimal("0.125"))).isEqualTo("0.125");
        assertThat(Money.quantity(new BigDecimal("1500"))).isEqualTo("1,500");
    }

    @Test
    void ratesRenderAsPercentages() {
        assertThat(Money.percent(new BigDecimal("0.1"))).isEqualTo("10%");
        assertThat(Money.percent(new BigDecimal("0.0825"))).isEqualTo("8.25%");
        assertThat(Money.percent(BigDecimal.ZERO)).isEqualTo("0%");
        assertThat(Money.percent(BigDecimal.ONE)).isEqualTo("100%");
    }

    @Test
    void addressLinesSkipBlanks() {
        PostalAddress address = new PostalAddress("1 Dock St", "", "Port City", null, "40010", "Freelandia");
        assertThat(address.lines()).isEqualTo(List.of("1 Dock St", "Port City 40010", "Freelandia"));
        assertThat(new PostalAddress(null, null, null, null, null, null).lines()).isEmpty();
    }
}
