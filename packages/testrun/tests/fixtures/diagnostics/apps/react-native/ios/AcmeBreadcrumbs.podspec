Pod::Spec.new do |s|
  s.name = 'AcmeBreadcrumbs'
  s.version = '1.0.0'
  s.summary = 'Fixture-owned bounded native breadcrumbs for Acme diagnostics.'
  s.homepage = 'https://example.com/acme'
  s.license = { :type => 'Proprietary', :text => 'Acme diagnostic fixture' }
  s.author = 'Acme'
  s.source = { :path => '.' }
  s.platforms = { :ios => '15.1' }
  s.source_files = 'AcmeBreadcrumbs.m'
  s.dependency 'React-Core'
end
