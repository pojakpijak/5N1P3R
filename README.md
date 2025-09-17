# 5N1P3R
The Solana Sniper - Basic version


Wprowadzenie do Testów w Środowisku Symulowanym MarketSimulator®.


Poniższa część dokumentacji definiuje procedurę uruchamiania i analizy testów dla bota 5N1P3R basic, przy użyciu zintegrowanego środowiska MarketSimulator®. Celem jest weryfikacja wydajności, stabilności i logiki biznesowej bota w kontrolowanych, powtarzalnych warunkach, które naśladują dynamikę sieci Solana.

Proces jest w pełni zautomatyzowany przez test_runner, który zarządza całym cyklem życia testu – od budowy komponentów, przez uruchomienie symulacji, aż po agregację wyników i generowanie raportu.


Faza 1: Przygotowanie Środowiska Testowego

1.1. Wymagania Wstępne
Przed rozpoczęciem upewnij się, że w Twoim środowisku systemowym ($PATH) dostępne są następujące narzędzia:
Rust Toolchain: Niezbędny do kompilacji bota i symulatora (cargo).
Solana CLI: Wymagany jest solana-test-validator do uruchomienia lokalnego klastra.

1.2. Struktura Projektu
Operujemy wewnątrz tego repozytorium. Kluczowe komponenty znajdują się w katalogu main/bot/src:
main.rs: Główny plik binarny bota.
src/bin/test_runner.rs: Nasze centrum dowodzenia testami.
src/bin/market_simulator.rs: Aplikacja symulująca rynek (TokenGenerator + MarketMaker).
test_config.toml: Plik konfiguracyjny dla test_runnera.

1.3. Konfiguracja Scenariuszy
Testowym, centralnym punktem konfiguracji jest plik: main/bot/test_config.toml. Definiuje on i ustawia przebieg całego testu.

Wymagania:
Ścieżka do solana-test-validator. Musi być w $PATH

Przykład konfiguracji:

solana_test_validator_path = "solana-test-validator"

# Ścieżki do crate'ów, które test_runner ma skompilować.
market_simulator_crate_path = "."  # Symulator jest w tym samym crate co runner
sniper_bot_crate_path = "."        # Bot również

# Definicja poszczególnych scenariuszy testowych
[[scenarios]]
name = "Standard Market Conditions"
duration_secs = 120
# ... inne parametry specyficzne dla scenariusza

[[scenarios]]
name = "High-Velocity Gem Rush (Stress Test)"
duration_secs = 60
# ... inne parametry specyficzne dla scenariusza.

Przed uruchomieniem testów, zweryfikuj poprawność ścieżek i dostosuj parametry scenariuszy (np. duration_secs) do swoich potrzeb.


Faza 2: Uruchomienie Procedury Testowej
Procedura jest w pełni zautomatyzowana. Wszystkie kroki są wykonywane przez test_runner.

2.1. Uruchomienie Test Runnera
Otwórz terminal w głównym katalogu bota (main/bot/) i wykonaj następujące polecenie:

cargo run --release --bin test_runner

To polecenie uruchomi proces testowy. Możesz również zdefiniować własną konfigurację scenariuszy i plik wyjściowy:

cargo run --release --bin test_runner -- --scenarios ./path/to/my_scenarios.toml --output ./path/to/my_results.json.


2.2. Przebieg Zautomatyzowanego Testu

Test Runner wykona następującą sekwencję operacji dla każdego scenariusza zdefiniowanego w pliku konfiguracyjnym:
Kompilacja Binarek: test_runner najpierw skompiluje market_simulator i sniffer_bot_light w trybie --release. Gwarantuje to, że testujemy zoptymalizowany, produkcyjny kod.

Uruchomienie Walidatora: W tle zostanie uruchomiony solana-test-validator z flagą --reset, zapewniając czyste, odizolowane środowisko dla każdego testu.

Uruchomienie Symulatora i Bota: market_simulator i sniffer_bot_light zostaną uruchomione jako osobne procesy, które natychmiast połączą się z lokalnym walidatorem.

Streaming Logów: stdout (w formacie JSON) i stderr obu procesów są na bieżąco zapisywane do dedykowanych plików w katalogu projektu (np. simulator_standard_market_conditions.stdout.jsonl), co zapobiega utracie danych i nadmiernemu zużyciu pamięci.

Przebieg Scenariusza: System działa przez zdefiniowany w scenariuszu czas (duration_secs). W tym czasie MarketSimulator generuje tokeny i aktywność rynkową, a SNIPER na nią reaguje.

Zakończenie i Sprzątanie: Po upływie czasu, test_runner bezpiecznie zakończy wszystkie procesy (simulator, bot, validator).


Faza 3: Analiza Wyników
Po zakończeniu wszystkich scenariuszy, test_runner automatycznie przetwarza zebrane logi i generuje finalny raport.

3.1. Raport w Konsoli

Dla każdego scenariusza na konsoli zostanie wyświetlone zwięzłe podsumowanie, zawierające kluczowe metryki wydajności:

--- Test Scenario Summary: 'High-Velocity Gem Rush (Stress Test)' ---
  Tokens Generated:
    - Gem     : 40
    - Rug     : 5
    - Trash   : 5
  Simulator Rug Pulls Executed: 5
  Bot Buy Attempts: 40
  Bot Buy Successes: 38
  Bot Success Rate: 95.00%
  Average Time-to-Execute (TTE): 152.10ms
  P95 Time-to-Execute (TTE): 220.00ms
  Errors Encountered: 2
    - [PROCESSEXITED]: Sniper Bot exited prematurely with status: exit status: 1
